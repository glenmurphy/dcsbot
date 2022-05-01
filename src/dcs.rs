use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

/**
 * Structs for serde to be able to deserialize the json
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
    pub SERVERS: Vec<Server>,
    //SERVERS_MAX_COUNT: i32,
    //SERVERS_MAX_DATE: String,
    //PLAYERS_COUNT: i32,
    //MY_SERVERS : Vec<Server>
}

pub enum ServersMessage {
    Servers(Servers),
    Versions((String, String)),
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
    login_headers.insert(
        "content-type",
        "application/x-www-form-urlencoded".parse().unwrap(),
    );

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
async fn get_servers(cookies: String) -> Result<Servers, String> {
    let mut headers = HeaderMap::new();
    headers.insert(reqwest::header::COOKIE, cookies.parse().unwrap());

    let servers_result = reqwest::Client::new()
        .get("https://www.digitalcombatsimulator.com/en/personal/server/?ajax=y")
        .headers(headers)
        .send()
        .await;

    match servers_result {
        Ok(servers) => match servers.json::<Servers>().await {
            Ok(json) => Ok(json),
            Err(err) => Err(format!("JSON parse error: {:?}", err)),
        },
        Err(err) => Err(format!("Load error: {:?}", err)),
    }
}

async fn parse_versions(text: String) -> Result<(String, String), String> {
    let mut lines = text.split("/en/news/changelog/openbeta/");
    let beta = match lines.nth(2) {
        Some(line) => line.split("/").nth(0).unwrap(),
        _ => return Err("Beta version not found".to_string()),
    };

    let mut lines = text.split("/en/news/changelog/stable/");
    let stable = match lines.nth(2) {
        Some(line) => line.split("/").nth(0).unwrap(),
        _ => return Err("Stable version not found".to_string()),
    };

    Ok((beta.to_string(), stable.to_string()))
}

async fn get_versions() -> Result<(String, String), String> {
    let versions_result = reqwest::Client::new()
        .get("https://www.digitalcombatsimulator.com/en/news/changelog/")
        .send()
        .await;

    match versions_result {
        Ok(versions) => match versions.text().await {
            Ok(text) => parse_versions(text).await,
            Err(err) => Err(format!("Text parse error: {:?}", err)),
        },
        Err(err) => Err(format!("Load error: {:?}", err)),
    }
}

async fn run_dcs(username: String, password: String, servers_tx: mpsc::Sender<ServersMessage>) {
    let cookies = login(username, password).await;
    if let Err(msg) = cookies {
        return println!("\x1b[31mLogin failed: {}\x1b[0m", msg);
    }

    let cookie_string = cookies.unwrap();
    let mut last_version_fetch = std::time::SystemTime::UNIX_EPOCH;

    loop {
        // Poll the DCS website every 3 hours to figure out what the latest
        // Open Beta and Stable version numbers are
        let now = std::time::SystemTime::now();
        if now.duration_since(last_version_fetch).unwrap().as_secs() > 60 * 60 * 3 {
            match get_versions().await {
                Ok(versions) => {
                    println!("Versions: {:?}", versions);
                    let _ = servers_tx.send(ServersMessage::Versions(versions)).await;
                    last_version_fetch = now;
                }
                Err(err) => println!("Version fetch error: {:?}", err),
            }
        }

        // Get the list of servers from the DCS website
        match get_servers(cookie_string.to_string()).await {
            Ok(servers) => {
                // As we are using regular channels instead of unbounded, this
                // will block if channel is full (max 1 message). This can be
                // caused if sending messages takes too long. We can consider
                // threading the sending of messages in bot, but this seems like
                // a reasonable rate limiter.
                let _ = servers_tx.send(ServersMessage::Servers(servers)).await;
            }
            Err(msg) => {
                println!("\x1b[31mFailed to get server list: {}\x1b[0m", msg);

                // Even though this might occur to due simple network errors,
                // we fall out of the loop so we can do a complete do-over,
                // in case the error is due to auth expiring
                return;
            }
        }

        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

pub async fn start(username: String, password: String, servers_tx: mpsc::Sender<ServersMessage>) {
    loop {
        run_dcs(username.clone(), password.clone(), servers_tx.clone()).await;

        // Only reaches this in case of failure - consider notifying and
        // exponential backoff
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
