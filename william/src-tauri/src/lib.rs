use rusqlite::params;
use tauri::async_runtime::spawn;

use chamber_common::{error, get_config_dir, get_local_dir, get_root_dir, lprint, Logger};
use dewey_lib::Dewey;

use crate::types::*;

mod network;
mod tiktoken;
mod types;

macro_rules! ws_send {
    ($ws:expr, $msg:expr) => {
        match $ws.write(tungstenite::Message::text($msg)) {
            Ok(_) => {
                $ws.flush().unwrap();
            }
            Err(e) => {
                error!("error writing to websocket: {}", e);
            }
        }
    };
}

// Shorthand for handling an error, and sending back a response
// NOTE: This continues after the response is sent, meaning it maintains the connection.
//       Use a pattern more akin to `serialize_response!` (e.g., panicking) if you need something
//       else
macro_rules! ws_error {
    ($ws:expr, $error_type:expr, $error_message:expr, $e:expr) => {
        let message = format!("{}: {}", $error_message, $e);
        lprint!(error, "{}", message);
        let response = serialize_response!(
            WilliamError,
            WilliamError {
                error_type: format!("{}", $error_type), // TODO: what do we put here?
                message
            }
        );

        ws_send!($ws, response);
    }
}

macro_rules! serialize_response {
    ($payload_type:ident, $payload:expr) => {
        match serde_json::to_string(&ArrakisResponse {
            payload: ResponsePayload::$payload_type($payload),
        }) {
            Ok(r) => r,
            Err(e) => {
                lprint!(error, "Error deserializing response: {}", e);
                panic!("William can't function with serde errors! Shutting down.");
            }
        }
    };
}

// Check if a directory exists, and create if needed
// Mainly just used in initialization
fn create_if_nonexistent(path: &std::path::PathBuf) {
    if !path.exists() {
        match std::fs::create_dir_all(&path) {
            Ok(_) => (),
            Err(e) => panic!("Failed to create directory: {:?}, {}", path, e),
        };
    }
}

fn get_embeddings_dir() -> std::path::PathBuf {
    get_local_dir().join("messages")
}

fn get_home() -> Option<String> {
    if cfg!(target_os = "windows") {
        // TODO: windows
        None
    } else {
        match std::env::var("HOME") {
            Ok(path) => Some(path),
            Err(_) => None,
        }
    }
}

// TODO: a lot of this setup code needs abstracted to a common module
//
// Sets up necessary config/local directories and touches required files to keep things from
// breaking/crashing on start up
fn setup() {
    // TODO: better path config handling
    let home_dir = match get_home() {
        Some(d) => d,
        None => {
            panic!("error: $HOME not set");
        }
    };

    let root = if cfg!(dev) {
        format!("{}/.local/william-dev", home_dir)
    } else {
        format!("{}/.local/william", home_dir)
    };

    chamber_common::Workspace::new(&root);

    create_if_nonexistent(&get_local_dir());
    create_if_nonexistent(&get_embeddings_dir());
    create_if_nonexistent(&get_config_dir());
    create_if_nonexistent(&get_root_dir().join("logs"));

    let log_name = if cfg!(dev) {
        "debug".to_string()
    } else {
        format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros()
        )
    };

    // TODO: proper logging, obviously
    chamber_common::Logger::init(
        get_root_dir()
            .join("logs")
            .join(format!("{}.log", log_name))
            .to_str()
            .unwrap(),
    );
}

fn is_valid_guid(guid: &str) -> bool {
    if guid.len() != 36 {
        return false;
    }

    if guid.chars().nth(8) != Some('-')
        || guid.chars().nth(13) != Some('-')
        || guid.chars().nth(18) != Some('-')
        || guid.chars().nth(23) != Some('-')
    {
        return false;
    }

    let hex_only: String = guid.chars().filter(|&c| c != '-').collect();

    if hex_only.len() != 32 {
        return false;
    }

    hex_only.chars().all(|c| c.is_ascii_hexdigit())
}

// DB initialization statement
// Creates the necessary tables and whatnot and is executed at start up each time
const DB_SETUP_STATEMENTS: &str = r#"
CREATE TABLE IF NOT EXISTS message_types (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

INSERT INTO message_types (name)
SELECT 'system'
WHERE NOT EXISTS (SELECT 1 FROM message_types WHERE name = 'system');

INSERT INTO message_types (name)
SELECT 'user'
WHERE NOT EXISTS (SELECT 1 FROM message_types WHERE name = 'user');

INSERT INTO message_types (name)
SELECT 'assistant'
WHERE NOT EXISTS (SELECT 1 FROM message_types WHERE name = 'assistant');

CREATE TABLE IF NOT EXISTS providers (
    name TEXT PRIMARY KEY
);

INSERT INTO providers (name)
SELECT 'openai'
WHERE NOT EXISTS (SELECT 1 FROM providers WHERE name = 'openai');

INSERT INTO providers (name)
SELECT 'groq'
WHERE NOT EXISTS (SELECT 1 FROM providers WHERE name = 'groq');

INSERT INTO providers (name)
SELECT 'anthropic'
WHERE NOT EXISTS (SELECT 1 FROM providers WHERE name = 'anthropic');

CREATE TABLE IF NOT EXISTS models (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    provider TEXT NOT NULL,
    FOREIGN KEY (provider) REFERENCES providers(name)
);

INSERT INTO models (name, provider)
SELECT 'gpt-4o', 'openai'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'gpt-4o' AND provider = 'openai');

INSERT INTO models (name, provider)
SELECT 'gpt-4o-mini', 'openai'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'gpt-4o-mini' AND provider = 'openai');

INSERT INTO models (name, provider)
SELECT 'o1-preview', 'openai'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'o1-preview' AND provider = 'openai');

INSERT INTO models (name, provider)
SELECT 'o1-mini', 'openai'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'o1-mini' AND provider = 'openai');

INSERT INTO models (name, provider)
SELECT 'llama3-70b-8192', 'groq'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'llama3-70b-8192' AND provider = 'groq');

INSERT INTO models (name, provider)
SELECT 'claude-3-opus-20240229', 'anthropic'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'claude-3-opus-20240229' AND provider = 'anthropic');

INSERT INTO models (name, provider)
SELECT 'claude-3-sonnet-20240229', 'anthropic'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'claude-3-sonnet-20240229' AND provider = 'anthropic');

INSERT INTO models (name, provider)
SELECT 'claude-3-haiku-20240307', 'anthropic'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'claude-3-haiku-20240307' AND provider = 'anthropic');

INSERT INTO models (name, provider)
SELECT 'claude-3-5-sonnet-latest', 'anthropic'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'claude-3-5-sonnet-latest' AND provider = 'anthropic');

INSERT INTO models (name, provider)
SELECT 'claude-3-5-haiku-latest', 'anthropic'
WHERE NOT EXISTS (SELECT 1 FROM models WHERE name = 'claude-3-5-haiku-latest' AND provider = 'anthropic');

CREATE TABLE IF NOT EXISTS conversations (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    message_type_id INTEGER NOT NULL,
    content TEXT NOT NULL,
    api_config_id INTEGER NOT NULL,
    system_prompt TEXT NOT NULL,
    FOREIGN KEY (message_type_id) REFERENCES message_types(id),
    FOREIGN KEY (api_config_id) REFERENCES api_configurations(id)
);

CREATE TABLE IF NOT EXISTS message_embeddings (
    id INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL,
    filepath TEXT NOT NULL,
    FOREIGN KEY (message_id) REFERENCES messages(id)
);

CREATE TABLE IF NOT EXISTS paths (
    id INTEGER PRIMARY KEY,
    conversation_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (message_id) REFERENCES messages(id)
);

CREATE TABLE IF NOT EXISTS forks (
    id INTEGER PRIMARY KEY,
    from_id INTEGER NOT NULL,
    to_id INTEGER NOT NULL,
    FOREIGN KEY (from_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (to_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_config (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    system_prompt TEXT,
    openai_key TEXT,
    groq_key TEXT,
    grok_key TEXT,
    anthropic_key TEXT,
    gemini_key TEXT
);
"#;

// TODO: optimize this
//       this should be done in batch
//
// TODO: there should probably be some decoupling
//       between Dewey and the SQLite db
//
// Embeds a message if it's not already embedded through Dewey
// TODO: What's the case in which it's already embedded?
fn add_message_embedding(
    // This is really gross
    // The Option<> gymnastics here are really just remarkably stupid
    dewey: &mut Option<&mut Dewey>,
    db: &rusqlite::Connection,
    message: &Message,
    filepath: &str,
) -> Result<(), std::io::Error> {
    if dewey.is_none() {
        lprint!(info, "Dewey unavailable, ignoring embedding request");
        return Ok(());
    }

    let dewey = dewey.as_mut();

    let exists: bool = db
        .query_row(
            "SELECT 1 FROM message_embeddings WHERE message_id = ?1 LIMIT 1",
            params![message.id],
            |_row| Ok(true),
        )
        .unwrap_or(false);

    if exists {
        return Ok(());
    }

    std::fs::write(filepath, message.content.clone())?;

    db.execute(
        "INSERT INTO message_embeddings (message_id, filepath) VALUES (?1, ?2)",
        params![message.id, filepath],
    )
    .unwrap();

    dewey.unwrap().add_embedding(filepath.to_string())?;

    Ok(())
}

// Basic prompt builder. Uses embedding memory and XML to structure prompts.
// TODO: This could probably be abstracted out to a more general prompt builder, but I can't see
//       the metastructure at the moment
fn build_system_prompt(
    conversation_len: usize,
    dewey_sources: &Vec<dewey_lib::EmbeddingSource>,
    tokenizer: Option<&tiktoken::Tokenizer>,
) -> String {
    let mut prompt = "<systemPrompt>".to_string();
    prompt.push_str(r#"
        <objective>
            Determine whether to use the following references to inform your response, and do so without explicitly acknowledging it.
            Incorporate into your judgment whether this moves the conversation forward, in the same direction as the user.
            If you decide to use it, do so in a friendly, familiar manner--leave what should stay unsaid, but implicitly acknowledge the history.
            If reasonable, try and use the references to fill in contextual gaps.
        </objective>
    "#);

    prompt.push_str("<references>");
    for source in dewey_sources {
        let prompt_len = if let Some(tok) = tokenizer {
            tok.encode(&prompt).len()
        } else {
            prompt.len()
        };

        if conversation_len + prompt_len > 128000 {
            break;
        }

        // TODO: error handling
        let contents = std::fs::read_to_string(&source.filepath).unwrap();

        let contents = contents[..std::cmp::min(512, contents.len())].to_string();
        prompt.push_str(&format!("<reference>{}</reference>", contents));
    }

    prompt.push_str("</references>");
    prompt.push_str("</systemPrompt>");

    prompt
}

// TODO: this needs to be accommodated for the high context windows
//
// Function to keep the conversation within context window limits. Returns the correct conversation
// history to use for the prompt.
fn cutoff_messages(
    messages: &Vec<Message>,
    tokenizer: Option<&tiktoken::Tokenizer>,
) -> (usize, Vec<Message>) {
    let mut cutoff = messages.len() - 1;
    let mut total_len = 0;
    for m in messages.iter().rev() {
        if m.content.is_empty() {
            continue;
        }

        total_len += if let Some(tok) = tokenizer {
            tok.encode(&m.content).len()
        } else {
            m.content.len()
        };

        // TODO: centralize context window limits for each model
        if total_len < 128000 {
            cutoff = std::cmp::max(0, cutoff - 1);
        }
    }

    (total_len, messages[cutoff..].to_vec())
}

fn generate_name(conversation: &mut Conversation) {
    // TODO: this needs to be async
    if is_valid_guid(&conversation.name) {
        let new_name = network::prompt(
            API::OpenAI(OpenAIModel::GPT4oMini),
            &r#"
            You will be given the start of a conversation.
            Give it a name.
            Guidelines:
            - No markdown
            - Respond with _only_ the name.
            "#
            .to_string(),
            &vec![conversation.messages[0].clone()],
        );

        conversation.name = new_name
            .unwrap()
            .content
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                c if c.is_alphanumeric() || c == '.' || c == '-' || c == ' ' => c,
                _ => '_',
            })
            .collect();
    }
}

// TODO: error handling for the results here
//
// NOTE: this _does not_ create a new message for the response
//       the last message in the conversation is expected to be
//       a placeholder to be filled here for the Assistant
fn completion(
    websocket: &mut tungstenite::WebSocket<std::net::TcpStream>,
    mut conversation: Conversation,
    tokenizer: Option<&tiktoken::Tokenizer>,
    db: &rusqlite::Connection,
    mut dewey: Option<&mut Dewey>,
) {
    generate_name(&mut conversation);

    // the conversation needs to be set with a db ID at this point
    conversation.upsert(db).unwrap();

    let (total_len, messages_payload) = cutoff_messages(&conversation.messages, tokenizer);

    // The conversation has to have at least one message from the user
    // TODO: This might change later
    let last_user_message = messages_payload
        .iter()
        .rev()
        .find(|m| m.message_type == MessageType::User)
        .unwrap();

    let api = last_user_message.api.clone();

    let filepath = get_embeddings_dir()
        .join(uuid::Uuid::new_v4().to_string())
        .to_string_lossy()
        .to_string();

    // TODO: error handling
    // TODO: system prompt building needs to be more fleshed out
    //       like, minimum sized system prompts?
    std::fs::write(&filepath, last_user_message.content.clone()).unwrap();
    let dewey_sources = if let Some(d) = dewey.as_mut() {
        match d.query(&filepath, Vec::new(), 10) {
            Ok(ds) => ds,
            Err(e) => {
                lprint!(
                    error,
                    "Error fetching references from Dewey: {}; ignoring",
                    e
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let system_prompt = build_system_prompt(total_len, &dewey_sources, tokenizer);

    // Update dewey with our message
    match add_message_embedding(&mut dewey, db, last_user_message, &filepath) {
        Ok(_) => {}
        Err(e) => {
            lprint!(error, "Error adding message to Dewey: {}; ignoring", e);
        }
    };

    // Separate thread to communicate with the LLM
    // Message deltas are streamed back through the channel
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        match network::prompt_stream(
            api,
            &messages_payload[..messages_payload.len() - 1].to_vec(),
            &system_prompt,
            tx,
        ) {
            Ok(_) => {}
            Err(e) => {
                error!("error sending message to GPT endpoint: {}", e);
                std::process::exit(1);
            }
        }
    });

    loop {
        match rx.recv() {
            Ok(message) => {
                // -2 to skip the last message, which is being filled by the active completion, and
                // get the last user message
                let request_id = conversation.messages[conversation.messages.len() - 2]
                    .id
                    .unwrap();

                // Update the last message with the delta
                // This is primarily for accurately storing things in the DB
                let last = conversation.messages.last_mut().unwrap();
                last.content.push_str(&message);

                // Make sure conversation metadata is correctly set
                let conversation_id = conversation.id.unwrap();
                let response_id = last.id.unwrap();
                let conversation_name = conversation.name.clone();

                ws_send!(
                    websocket,
                    serialize_response!(
                        Completion,
                        Completion {
                            stream: true,
                            delta: message,
                            name: conversation_name,
                            conversation_id,
                            request_id,
                            response_id,
                        }
                    )
                );
            }
            // TODO: this feels disgusting. There has to be a better way of telling when the stream
            //       has ended
            Err(e) => {
                lprint!(info, "Assuming stream completed... ({})", e);

                // Weird one-off response serialization
                let response = match serde_json::to_string(&ArrakisResponse {
                    payload: ResponsePayload::CompletionEnd,
                }) {
                    Ok(r) => r,
                    Err(e) => {
                        lprint!(error, "Error deserializing response: {}", e);
                        panic!("William can't function with serde errors! Shutting down.");
                    }
                };

                ws_send!(websocket, response);

                // Backend storage duties--SQLite + embedding generation/storage

                match conversation.upsert(db) {
                    Ok(_) => {}
                    Err(e) => {
                        ws_error!(
                            websocket,
                            "Completion",
                            "Error upserting conversation in DB",
                            e
                        );
                    }
                };

                if dewey.is_some() {
                    match add_message_embedding(
                        &mut dewey,
                        db,
                        conversation.messages.last().unwrap(),
                        &filepath,
                    ) {
                        Ok(_) => {}
                        Err(e) => {
                            lprint!(error, "Error adding message to Dewey: {}; ignoring", e);
                        }
                    };
                }

                break;
            }
        }
    }
}

// Fetch a whole conversation from SQLite with a given ID
fn get_conversation(conversation_id: i64, db: &rusqlite::Connection) -> Conversation {
    let mut query = db
        .prepare(
            "
            SELECT
                c.id as conversation_id,
                c.name as conversation_name,
                m.id as message_id,
                m.message_type_id,
                m.content,
                api.provider,
                api.name,
                m.system_prompt,
                l.sequence
            FROM conversations c
            JOIN paths l ON c.id = l.conversation_id
            JOIN messages m ON l.message_id = m.id
            JOIN models api ON m.api_config_id = api.id
            WHERE c.id = ?1
            ORDER BY l.sequence ASC
            ",
        )
        .unwrap();

    let rows = query
        .query_map(params![conversation_id], |row| {
            let provider = row.get::<_, String>("provider")?;
            let model_name = row.get::<_, String>("name")?;
            let api = API::from_strings(&provider, &model_name)
                .map_err(|e| rusqlite::Error::InvalidParameterName(e))?;

            Ok((
                row.get::<_, i64>("conversation_id")?,
                row.get::<_, String>("conversation_name")?,
                row.get::<_, i64>("message_id")?,
                MessageType::from_id(row.get::<_, i64>("message_type_id")?).unwrap(),
                row.get::<_, String>("content")?,
                api,
                row.get::<_, String>("system_prompt")?,
                row.get::<_, i32>("sequence")?,
            ))
        })
        .unwrap();

    let mut conversation = Conversation {
        id: Some(conversation_id),
        name: String::new(),
        messages: Vec::new(),
    };

    for row in rows {
        let row = row.unwrap();
        conversation.name = row.1;
        conversation.messages.push(Message {
            id: Some(row.2),
            message_type: row.3,
            content: row.4,
            api: row.5,
            system_prompt: row.6,
            sequence: row.7,
        });
    }

    conversation
}

// Get the user config, or the prepared defaults
// It really feels gross to insert a default every time we want to fetch the config
fn get_config(db: &rusqlite::Connection) -> UserConfig {
    match db.execute("INSERT OR IGNORE INTO user_config (openai_key, groq_key, grok_key, anthropic_key, gemini_key, system_prompt) 
                      VALUES ('', '', '', '', '', '')", params![]) {
        Ok(_) => {},
        Err(e) => {
            lprint!(error, "Error setting user_config defaults: {}", e);
            panic!("Error setting user_config defaults: {}", e);
        }
    };

    let mut stmt = db
        .prepare(
            "SELECT openai_key, groq_key, grok_key, anthropic_key, gemini_key, system_prompt
                                 FROM user_config LIMIT 1",
        )
        .unwrap();

    let config = stmt
        .query_row(params![], |row| {
            Ok(UserConfig {
                write: false,
                api_keys: APIKeys {
                    openai: row.get(0)?,
                    groq: row.get(1)?,
                    grok: row.get(2)?,
                    anthropic: row.get(3)?,
                    gemini: row.get(4)?,
                },
                system_prompt: row.get(5)?,
            })
        })
        .unwrap();

    return config;
}

fn register_env_var(env_var: &str, value: &str) {
    std::env::set_var(env_var, value);
    lprint!(
        info,
        "{}: {}",
        env_var,
        if value.len() > 0 { "***" } else { "" }
    );
}

// TODO: there is zero error handling around here lol
async fn websocket_server() {
    setup();
    lprint!(info, "Workspace initialized");

    // Tokenizer using the GPT-4o token mapping from OpenAI
    let tokenizer_ = std::sync::Arc::new(std::sync::Mutex::new(
        match tiktoken::Tokenizer::new().await {
            Ok(t) => Some(t),
            Err(e) => {
                lprint!(error, "Error initializing tokenizer: {}; ignoring...", e);
                None
            }
        },
    ));

    lprint!(info, "Tokenizer initialized");

    // The SQLite database is used to store conversations/messages + the like
    // Probably want a more detailed description here
    let db_ = std::sync::Arc::new(std::sync::Mutex::new(
        rusqlite::Connection::open(get_local_dir().join("william.sqlite"))
            .expect("Failed to open database"),
    ));

    lprint!(info, "SQLite connection established");

    // DB initialization
    db_.lock()
        .unwrap()
        .execute_batch(DB_SETUP_STATEMENTS)
        .expect("Failed to initialize database");

    lprint!(info, "SQLite database initialized");

    lprint!(info, "Setting environment variables...");
    let user_config = get_config(&db_.lock().unwrap());
    register_env_var("OPENAI_API_KEY", &user_config.api_keys.openai);
    register_env_var("ANTHROPIC_API_KEY", &user_config.api_keys.anthropic);
    register_env_var("GEMINI_API_KEY", &user_config.api_keys.gemini);
    register_env_var("GROQ_API_KEY", &user_config.api_keys.groq);

    lprint!(info, "Environment variables set");

    // Embeddings are retrieved from the OpenAI API and stored locally using Dewey as the index
    let dewey_ = std::sync::Arc::new(std::sync::Mutex::new(match dewey_lib::Dewey::new() {
        Ok(d) => Some(d),
        Err(e) => {
            lprint!(error, "Error initializing Dewey: {}; ignoring...", e);
            None
        }
    }));

    lprint!(info, "Dewey initialized");

    let server = match std::net::TcpListener::bind("127.0.0.1:9001") {
        Ok(s) => s,
        Err(e) => {
            lprint!(error, "Error binding websocket: {}", e);
            return;
        }
    };

    lprint!(info, "WebSocket server listening on ws://127.0.0.1:9001");

    // Websocket server loop
    for stream in server.incoming() {
        let tokenizer = std::sync::Arc::clone(&tokenizer_);
        let db = std::sync::Arc::clone(&db_);
        let dewey = std::sync::Arc::clone(&dewey_);
        std::thread::spawn(move || {
            let stream = stream.unwrap();
            let mut websocket = tungstenite::accept(stream).unwrap();

            loop {
                let msg = match websocket.read() {
                    Ok(m) => m,
                    Err(e) => {
                        error!("error reading from websocket: {}", e);
                        continue;
                    }
                };

                let request: ArrakisRequest = match msg {
                    tungstenite::Message::Close(_) => {
                        break;
                    }
                    tungstenite::Message::Text(t) => match serde_json::from_str(&t) {
                        Ok(r) => r,
                        Err(e) => {
                            error!("t: {}", t);
                            error!("error reading Arrakis request: {}", e);
                            continue;
                        }
                    },
                    _ => {
                        error!("unsupported message type");
                        continue;
                    }
                };

                lprint!(info, "Request deserialized");

                // Not sure if there is a better way of delineating endpoints, but this is the best
                // we have right now.
                //
                // Request types are judged by their payload structure--see `types.rs` for more
                // info.
                match request {
                    // Triggers on a chat message submission, as well as a fork
                    // (after backend processing)
                    ArrakisRequest::Completion { payload } => {
                        completion(
                            &mut websocket,
                            payload,
                            tokenizer.lock().unwrap().as_ref(),
                            &db.lock().unwrap(),
                            dewey.lock().unwrap().as_mut(),
                        );
                    }
                    // TODO: Not sure how necessary this is
                    ArrakisRequest::Ping { payload: _ } => {
                        ws_send!(
                            websocket,
                            serialize_response!(
                                Ping,
                                Ping {
                                    body: "pong".to_string(),
                                }
                            )
                        );
                    }
                    // Retrieve a list of saved conversation IDs
                    ArrakisRequest::ConversationList => {
                        let db = db.lock().unwrap();
                        let mut query = db.prepare("SELECT id, name from conversations").unwrap();
                        let conversations = match query.query_map(params![], |row| {
                            Ok(Conversation {
                                id: row.get(0)?,
                                name: row.get(1)?,
                                messages: Vec::new(),
                            })
                        }) {
                            Ok(q) => q,
                            Err(e) => {
                                ws_error!(
                                    websocket,
                                    "ConversationList",
                                    "Error fetching conversation IDs",
                                    e
                                );
                                continue;
                            }
                        }
                        .map(|c| c.unwrap())
                        .collect();

                        ws_send!(
                            websocket,
                            serialize_response!(
                                ConversationList,
                                ConversationList { conversations }
                            )
                        );
                    }
                    // Fetch a conversation from its ID
                    ArrakisRequest::Load { payload } => {
                        ws_send!(
                            websocket,
                            serialize_response!(
                                Load,
                                get_conversation(payload.id, &db.lock().unwrap()).into()
                            )
                        );
                    }
                    // Read or write to the saved system prompt, depending on the request
                    ArrakisRequest::SystemPrompt { payload } => {
                        let path = get_config_dir().join("system_prompt");

                        if payload.write {
                            match std::fs::write(path.clone(), payload.content) {
                                Ok(_) => {
                                    lprint!(
                                        info,
                                        "system prompt saved to {}",
                                        path.to_str().unwrap(),
                                    );
                                }
                                Err(e) => {
                                    ws_error!(
                                        websocket,
                                        "SystemPrompt",
                                        "Error saving system prompt",
                                        e
                                    );
                                    continue;
                                }
                            };

                            continue;
                        }

                        let content = match std::fs::read_to_string(path.clone()) {
                            Ok(c) => c,
                            Err(e) => {
                                ws_error!(
                                    websocket,
                                    "SystemPrompt",
                                    "error reading system prompt file {}: {}",
                                    e
                                );
                                continue;
                            }
                        };

                        ws_send!(
                            websocket,
                            serialize_response!(
                                SystemPrompt,
                                SystemPrompt {
                                    write: false,
                                    content,
                                }
                            )
                        );
                    }
                    // get the current conversation,
                    // create the fork,
                    // carry on with the completion
                    //
                    // TODO: this needs cleaned up from a UI perspective. Regenerating messages
                    //       when there's a communication failure quickly leads to a cluttering of the
                    //       conversation history. They also need renamed based on the conversation
                    //       redirection
                    ArrakisRequest::Fork { payload } => {
                        let db = db.lock().unwrap();

                        let mut conversation = get_conversation(payload.conversation_id, &db);

                        conversation.id = None;
                        conversation.name = format!("Fork: {}", conversation.name);
                        conversation.messages = conversation
                            .messages
                            .iter()
                            .take(payload.sequence as usize)
                            .cloned()
                            .collect();

                        // The conversation should _always_ have at least one element--what would
                        // there be to fork otherwise?
                        let mut assistant_message = conversation.messages.last().unwrap().clone();

                        if assistant_message.message_type != MessageType::Assistant {
                            assistant_message.id = None;
                            assistant_message.message_type = MessageType::Assistant;
                            assistant_message.content = String::new();
                            assistant_message.sequence += 1;

                            conversation.messages.push(assistant_message);
                        } else {
                            let last = conversation.messages.last_mut().unwrap();
                            last.content = String::new();
                        }

                        let _ = conversation.upsert(&db);
                        let new_id = db.last_insert_rowid();

                        let fork_query = "INSERT INTO forks (from_id, to_id) VALUES (?, ?)";
                        match db.execute(fork_query, params![payload.conversation_id, new_id]) {
                            Ok(_) => {}
                            Err(e) => {
                                ws_error!(websocket, "Fork", "Error adding fork to DB", e);
                                continue;
                            }
                        };

                        completion(
                            &mut websocket,
                            conversation,
                            tokenizer.lock().unwrap().as_ref(),
                            &db,
                            dewey.lock().unwrap().as_mut(),
                        )
                    }
                    ArrakisRequest::Config { payload } => {
                        println!("Received Config request");

                        let db = db.lock().unwrap();

                        let config = get_config(&db);

                        if payload.write {
                            let mut update_stmt = db
                                .prepare(
                                    "UPDATE user_config 
                                     SET openai_key = ?1, 
                                         groq_key = ?2, 
                                         grok_key = ?3, 
                                         anthropic_key = ?4, 
                                         gemini_key = ?5, 
                                         system_prompt = ?6",
                                )
                                .unwrap();

                            match update_stmt.execute(params![
                                payload.api_keys.openai,
                                payload.api_keys.groq,
                                payload.api_keys.grok,
                                payload.api_keys.anthropic,
                                payload.api_keys.gemini,
                                payload.system_prompt,
                            ]) {
                                Ok(_) => {}
                                Err(e) => {
                                    ws_error!(websocket, "Config", "Error updating user config", e);
                                    continue;
                                }
                            };
                        } else {
                            ws_send!(websocket, serialize_response!(Config, config));
                        }
                    }
                    ArrakisRequest::WilliamError { payload: _ } => {
                        // There shouldn't be any requests for this type
                    }
                };
            }
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_| {
            spawn(async move {
                websocket_server().await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
