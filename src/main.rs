use dirs;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
// use rustix::process;
use chrono;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::{
    env,
    fs::{self},
    io::{Error, Read},
};

#[derive(Serialize, Deserialize, Debug)]
struct Log {
    role: String,
    content: String,
    tokens: i64,
}

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    role: String,
    content: String,
}
#[derive(Debug, Deserialize, Serialize)]
struct OpenAIRequest {
    #[serde(rename = "model")]
    model: String,
    #[serde(rename = "messages")]
    messages: Vec<Message>,
}

fn get_latest_file(folder_path: &PathBuf) -> PathBuf {
    let mut latest_file: PathBuf = PathBuf::new();
    let mut latest_file_time: u64 = 0;
    for entry in fs::read_dir(folder_path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let metadata = fs::metadata(&path).unwrap();
        let modified = metadata.modified().unwrap();
        let modified_time = modified
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if modified_time > latest_file_time {
            latest_file_time = modified_time;
            latest_file = path;
        }
    }
    latest_file
}

fn main() -> Result<(), Error> {
    // get OPENAI_API_KEY from environment variable
    let key = "OPENAI_API_KEY";
    let openai_api_key = env::var(key).expect(&format!("{} not set", key));

    // get the prompt from the user
    let args: Vec<String> = env::args().skip(1).collect();
    let prompt = args.join(" ");
    if args[0] == "reset" {
        let current_timestamp_in_seconds = chrono::Utc::now().timestamp();
        // create a new chatlog file with the current timestamp
        let chatgpt_folder: PathBuf = dirs::home_dir()
            .expect("Failed to get home directory")
            .join(".chatgpt");
        let chatlog_path = chatgpt_folder.join(format!("{}.json", current_timestamp_in_seconds));
        fs::File::create(chatlog_path)?;
        println!(
            "Created new chatlog file: {}.json",
            current_timestamp_in_seconds
        );
        return Ok(());
    }

    let folder_path: PathBuf = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".chatgpt");

    let chatlog_path = get_latest_file(&folder_path);

    let (mut chatlog, data) = create_request(&chatlog_path, &prompt)?;
    let client = Client::new();

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", openai_api_key).parse().unwrap(),
    );
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    let json_data = serde_json::to_string(&data)?;

    let start_time_seconds = chrono::Utc::now().timestamp();
    let response = client
        .post("https://api.openai.com/v1/chat/completions".to_string())
        .headers(headers)
        .body(json_data)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();

    // if the response is an error, print it and exit
    match response["error"].as_object() {
        None => response["error"].clone(),
        Some(_) => {
            println!(
                "Received an error from OpenAI: {}",
                response["error"]["message"].as_str().unwrap()
            );
            return Ok(());
        }
    };

    let prompt_tokens = response["usage"]["prompt_tokens"].as_i64().unwrap();
    let answer_tokens = response["usage"]["completion_tokens"].as_i64().unwrap();
    let answer = response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();

    // Show the response from OpenAI
    println!("{}", answer);

    // save the new messages to the chatlog
    chatlog.push(Log {
        role: "user".to_string(),
        content: prompt,
        tokens: prompt_tokens,
    });
    chatlog.push(Log {
        role: "assistant".to_string(),
        content: answer.to_string(),
        tokens: answer_tokens,
    });

    // write the chatlog to disk
    let chatlog_text = serde_json::to_string(&chatlog)?;
    fs::write(&chatlog_path, chatlog_text)?;

    let end_time_seconds = chrono::Utc::now().timestamp();
    let elapsed_time_seconds = end_time_seconds - start_time_seconds;
    println!("Elapsed time: {} seconds", elapsed_time_seconds.to_string());
    Ok(())
}

fn create_request(
    chatlog_path: &PathBuf,
    prompt: &String,
) -> Result<(Vec<Log>, OpenAIRequest), Error> {
    let mut file = OpenOptions::new()
        .create(true) // create the file if it doesn't exist
        .append(true) // don't overwrite the contents
        .read(true)
        .open(chatlog_path)
        .unwrap();
    let mut chatlog_text = String::new();
    file.read_to_string(&mut chatlog_text)?;
    const MAX_TOKENS: i64 = 2000;
    let mut total_tokens: i64 = 0;
    let mut messages: Vec<Message> = vec![];
    let mut chatlog: Vec<Log> = vec![];
    if !chatlog_text.is_empty() {
        chatlog = serde_json::from_str(&chatlog_text)?;
        for log in chatlog.iter().rev() {
            if total_tokens + log.tokens > MAX_TOKENS {
                println!("Trunkating chatlog to {} tokens", MAX_TOKENS);
                break;
            }

            total_tokens += log.tokens;
            messages.push(Message {
                role: log.role.clone(),
                content: log.content.clone(),
            });
        }
    }
    messages = messages.into_iter().rev().collect();
    messages.push(Message {
        role: "user".to_string(),
        content: prompt.clone(),
    });
    let data = OpenAIRequest {
        model: "gpt-3.5-turbo".to_string(),
        messages,
    };
    Ok((chatlog, data))
}
