use reqwest::header::HeaderMap;
use serde::{Deserialize};
use tokio::sync::mpsc::UnboundedSender;
use std::time::Duration;

/**
 * Structs for serde to be able to deserealize the json
 */
#[derive(Deserialize, Clone, Debug)]
#[allow(non_snake_case)]
pub struct Server {
    pub NAME: String,
    pub MISSION_NAME: String,
    pub PLAYERS: String,

    pub IP_ADDRESS: String,
    pub PORT: String,

    pub DCS_VERSION: String,

    //MISSION_TIME: String,
    //PLAYERS_MAX: String,
    //PASSWORD: String,
    //DESCRIPTION: String,
    //MISSION_TIME_FORMATTED: String,
}

#[derive(Deserialize, Clone, Debug)]
#[allow(non_snake_case)]
pub struct Servers {
    pub SERVERS : Vec<Server>,

    //SERVERS_MAX_COUNT: i32,
    //SERVERS_MAX_DATE: String,
    //PLAYERS_COUNT: i32,
    //MY_SERVERS : Vec<Server>
}

pub enum ServersMessage {
    Servers(Servers)
}

/**
 * As get_all("set-cookie") doesn't work, we have to manually parse the separate
 * set-cookie lines into a single cookie string.
 */
fn parse_cookie(headers: &HeaderMap) -> String {
    let mut cookies = vec![];
    for (key, value) in headers.iter() {
        if key == "set-cookie" {
            cookies.push(value.to_str().unwrap())
        }
    }
    cookies.join(", ")
}

/**
 * Gets a login cookie from the DCS website
 */
async fn login(username: String, password: String) -> Result<String, &'static str> {
    if username.is_empty() || password.is_empty() {
        return Err("No username or password");
    }

    let mut login_headers = HeaderMap::new();
    login_headers.insert("content-type", "application/x-www-form-urlencoded".parse().unwrap());

    let client = reqwest::Client::new();
    let res = client.post("https://www.digitalcombatsimulator.com/en/")
        .headers(login_headers)
        .body(format!("AUTH_FORM=Y&TYPE=AUTH&backurl=%2Fen%2F&USER_LOGIN={}&USER_PASSWORD={}&USER_REMEMBER=Y&Login=Authorize", username, password))
        .send().await
        .unwrap();

    let cookies = parse_cookie(res.headers());
    if !cookies.contains("BITRIX_SM_UIDL=") {
        return Err("username/password incorrect");
    }

    Ok(cookies)
}

/**
 * Gets the current list of servers from the DCS website
 */
async fn get_servers(cookies: String) -> Result<Servers, &'static str> {
    let mut headers = HeaderMap::new();
    headers.insert(reqwest::header::COOKIE, cookies.parse().unwrap());

    let client = reqwest::Client::new();
    let servers_result = client.get("https://www.digitalcombatsimulator.com/en/personal/server/?ajax=y")
        .headers(headers)
        .send()
        .await;
    
    match servers_result {
        Ok(servers) => {
            let json_result = servers.json::<Servers>().await;
            match json_result {
                Ok(json) => Ok(json),
                Err(_) => Err("JSON parse error")
            }
        },
        Err(_) => Err("Load error")
    }
}

pub async fn main(username: String, password: String, servers_tx: UnboundedSender<ServersMessage>) {
    let cookies = login(username, password).await;
    if let Err(msg) = cookies {
        println!("\x1b[31mLogin failed: {}\x1b[0m", msg);
        return
    }

    let cookie_string = cookies.unwrap();

    tokio::spawn(async move {
        loop {
            match get_servers(cookie_string.to_string()).await {
                Ok(servers) => {
                    let _ = servers_tx.send(ServersMessage::Servers(servers));
                },
                Err(msg) => {
                    println!("\x1b[31mFailed to get server list: {}\x1b[0m", msg);
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}