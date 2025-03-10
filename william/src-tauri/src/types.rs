use rusqlite::params;

#[derive(PartialEq, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum MessageType {
    System,
    User,
    Assistant,
    Developer,
}

impl MessageType {
    pub fn to_string(&self) -> String {
        match self {
            MessageType::System => "system".to_string(),
            MessageType::User => "user".to_string(),
            MessageType::Assistant => "assistant".to_string(),
            MessageType::Developer => "developer".to_string(),
        }
    }

    pub fn id(&self) -> i64 {
        match self {
            MessageType::System => 0,
            MessageType::User => 1,
            MessageType::Assistant => 2,
            MessageType::Developer => 2,
        }
    }

    pub fn from_id(id: i64) -> Result<Self, String> {
        match id {
            0 => Ok(MessageType::System),
            1 => Ok(MessageType::User),
            2 => Ok(MessageType::Assistant),
            3 => Ok(MessageType::Developer),
            _ => Err(format!("Invalid message type: {}", id)),
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, Hash, Eq, PartialEq)]
#[serde(tag = "provider", content = "model")]
pub enum API {
    #[serde(rename = "openai")]
    OpenAI(OpenAIModel),
    #[serde(rename = "groq")]
    Groq(GroqModel),
    #[serde(rename = "anthropic")]
    Anthropic(AnthropicModel),
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, Hash, Eq, PartialEq)]
pub enum OpenAIModel {
    #[serde(rename = "gpt-4o")]
    GPT4o,
    #[serde(rename = "gpt-4o-mini")]
    GPT4oMini,
    #[serde(rename = "o1-preview")]
    O1Preview,
    #[serde(rename = "o1-mini")]
    O1Mini,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, Hash, Eq, PartialEq)]
pub enum GroqModel {
    #[serde(rename = "llama3-70b-8192")]
    LLaMA70B,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, Hash, Eq, PartialEq)]
pub enum AnthropicModel {
    #[serde(rename = "claude-3-opus-20240229")]
    Claude3Opus,
    #[serde(rename = "claude-3-sonnet-20240229")]
    Claude3Sonnet,
    #[serde(rename = "claude-3-haiku-20240307")]
    Claude3Haiku,
    #[serde(rename = "claude-3-5-sonnet-latest")]
    Claude35Sonnet,
    #[serde(rename = "claude-3-7-sonnet-20250219")]
    Claude37Sonnet,
    #[serde(rename = "claude-3-5-haiku-latest")]
    Claude35Haiku,
}

impl API {
    pub fn from_strings(provider: &str, model: &str) -> Result<Self, String> {
        match provider {
            "openai" => {
                let model = match model {
                    "gpt-4o" => OpenAIModel::GPT4o,
                    "gpt-4o-mini" => OpenAIModel::GPT4oMini,
                    "o1-preview" => OpenAIModel::O1Preview,
                    "o1-mini" => OpenAIModel::O1Mini,
                    _ => return Err(format!("Unknown OpenAI model: {}", model)),
                };
                Ok(API::OpenAI(model))
            }
            "groq" => {
                let model = match model {
                    "llama3-70b-8192" => GroqModel::LLaMA70B,
                    _ => return Err(format!("Unknown Groq model: {}", model)),
                };
                Ok(API::Groq(model))
            }
            "anthropic" => {
                let model = match model {
                    "claude-3-opus-20240229" => AnthropicModel::Claude3Opus,
                    "claude-3-sonnet-20240229" => AnthropicModel::Claude3Sonnet,
                    "claude-3-haiku-20240307" => AnthropicModel::Claude3Haiku,
                    "claude-3-5-sonnet-latest" => AnthropicModel::Claude35Sonnet,
                    "claude-3-5-haiku-latest" => AnthropicModel::Claude35Haiku,
                    _ => return Err(format!("Unknown Anthropic model: {}", model)),
                };
                Ok(API::Anthropic(model))
            }
            _ => Err(format!("Unknown provider: {}", provider)),
        }
    }

    /// Returns a tuple of (provider, model)
    pub fn to_strings(&self) -> (String, String) {
        match self {
            API::OpenAI(model) => {
                let model_str = match model {
                    OpenAIModel::GPT4o => "gpt-4o",
                    OpenAIModel::GPT4oMini => "gpt-4o-mini",
                    OpenAIModel::O1Preview => "o1-preview",
                    OpenAIModel::O1Mini => "o1-mini",
                };
                ("openai".to_string(), model_str.to_string())
            }
            API::Groq(model) => {
                let model_str = match model {
                    GroqModel::LLaMA70B => "llama3-70b-8192",
                };
                ("groq".to_string(), model_str.to_string())
            }
            API::Anthropic(model) => {
                let model_str = match model {
                    AnthropicModel::Claude3Opus => "claude-3-opus-20240229",
                    AnthropicModel::Claude3Sonnet => "claude-3-sonnet-20240229",
                    AnthropicModel::Claude3Haiku => "claude-3-haiku-20240307",
                    AnthropicModel::Claude35Sonnet => "claude-3-5-sonnet-latest",
                    AnthropicModel::Claude35Haiku => "claude-3-5-haiku-latest",
                    AnthropicModel::Claude37Sonnet => "claude-3-5-sonnet-latest",
                };
                ("anthropic".to_string(), model_str.to_string())
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: Option<i64>,
    pub message_type: MessageType,
    pub content: String,
    pub api: API,
    pub system_prompt: String,
    pub sequence: i32,
    pub date_created: String,
}

impl Message {
    pub fn update(&self, db: &rusqlite::Connection) -> rusqlite::Result<usize> {
        db.execute(
            "UPDATE messages SET content = ?2, system_prompt = ?3 WHERE id = ?1",
            params![self.id, self.content, self.system_prompt],
        )
    }

    pub fn insert(&mut self, db: &rusqlite::Connection) -> rusqlite::Result<usize> {
        let (provider, model_name) = self.api.to_strings();

        let api_config_id: i64 = db.query_row(
            "SELECT id FROM models WHERE provider = ?1 AND name = ?2",
            params![provider, model_name],
            |row| row.get(0),
        )?;

        let update_count = db.execute(
            "INSERT INTO messages (message_type_id, content, api_config_id, system_prompt, date_created) VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
            params![
                self.message_type.id(),
                self.content,
                api_config_id,
                self.system_prompt
            ],
        )?;

        self.id = Some(db.last_insert_rowid());

        Ok(update_count)
    }

    pub fn upsert(&mut self, db: &rusqlite::Connection) -> rusqlite::Result<usize> {
        if self.update(db)? == 0 {
            self.insert(db)
        } else {
            Ok(1)
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Conversation {
    pub id: Option<i64>,
    pub name: String,
    pub messages: Vec<Message>,
}

impl Conversation {
    // the IDs of all objects _need_ to be set before leaving this function
    // basic order here is something like:
    // - upsert conversation table
    // - upsert each message item (for setting IDs + updating contents)
    // - reset paths
    pub fn upsert(&mut self, db: &rusqlite::Connection) -> rusqlite::Result<usize> {
        if self.id.is_none() {
            db.execute(
                "INSERT INTO conversations (name, last_updated, date_created) VALUES (?1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
                params![self.name],
            )?;

            self.id = Some(db.last_insert_rowid());
        } else {
            db.execute(
                "UPDATE conversations SET name = ?2, last_updated = CURRENT_TIMESTAMP WHERE id = ?1",
                params![self.id, self.name],
            )?;
        }

        for message in self.messages.iter_mut() {
            message.upsert(db)?;
        }

        db.execute(
            "DELETE FROM paths WHERE conversation_id = ?1",
            params![self.id],
        )?;

        for (sequence, message) in self.messages.iter().enumerate() {
            db.execute(
                "INSERT INTO paths (conversation_id, message_id, sequence) VALUES (?1, ?2, ?3)",
                params![self.id, message.id, sequence as i64],
            )?;
        }

        Ok(1)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConversationList {
    pub conversations: Vec<Conversation>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LoadConversation {
    pub id: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Ping {
    pub body: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Fork {
    #[serde(rename = "conversationId")]
    pub conversation_id: i64,
    pub sequence: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct APIKeys {
    pub openai: String,
    pub groq: String,
    pub grok: String,
    pub anthropic: String,
    pub gemini: String,
}

// Represents the state of the user's configured settings and secrets
// Ideally this will stay as small as possible
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UserConfig {
    pub write: bool,
    #[serde(rename = "apiKeys")]
    pub api_keys: APIKeys,
    #[serde(rename = "systemPrompt")]
    pub system_prompt: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Preview {
    #[serde(rename = "conversationId")]
    pub conversation_id: i64,
    pub content: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeleteConversation {
    #[serde(rename = "conversationId")]
    pub conversation_id: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UsageRequest {
    #[serde(rename = "conversationId")]
    pub conversation_id: Option<i64>,
    pub api: API,
    #[serde(rename = "dateFrom")]
    pub date_from: String,
    #[serde(rename = "dateTo")]
    pub date_to: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RequestPayload {
    Ping(Ping),
    Completion(Conversation),
    ConversationList,
    Load(LoadConversation),
    Fork(Fork),
    Config(UserConfig),
    Preview(Preview),
    DeleteConversation(DeleteConversation),
    Usage(UsageRequest),
}

/// Request in JSON form looks like
/// {
///     "method": <request enum type>,
///     "payload": {
///         <request payload JSON>
///     }
/// }
///
/// e.g.,
/// // Update configuration
/// {
///   "method": "Config",
///   "payload": {
///     "write": true,
///     "apiKeys": {
///       "openai": "sk-...",
///       "groq": "gsk-...",
///       "grok": "...",
///       "anthropic": "sk-ant-...",
///       "gemini": "..."
///     },
///     "systemPrompt": "You are a helpful assistant"
///   }
/// }
///
/// To add new request types:
/// 1. Add a new struct for the payload type
/// 2. Add a new variant the `RequestPayload`
/// 3. Add a new variant the `ArrakisRequest`
///
/// This process is the same for responses
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "method")]
pub enum ArrakisRequest {
    Ping {
        id: String,
        payload: Ping,
    },
    Completion {
        id: String,
        payload: Conversation,
    },
    ConversationList {
        id: String,
    },
    Load {
        id: String,
        payload: LoadConversation,
    },
    Fork {
        id: String,
        payload: Fork,
    },
    Config {
        id: String,
        payload: UserConfig,
    },
    WilliamError {
        id: String,
        payload: WilliamError,
    },
    Preview {
        id: String,
        payload: Preview,
    },
    DeleteConversation {
        id: String,
        payload: DeleteConversation,
    },
    Usage {
        id: String,
        payload: UsageRequest,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ResponsePayload {
    Ping(Ping),
    Completion(Completion),
    CompletionEnd(SystemPrompt),
    ConversationList(ConversationList),
    Load(Conversation),
    Config(UserConfig),
    WilliamError(WilliamError),
    Preview(Preview),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WilliamError {
    pub error_type: String,
    pub message: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SystemPrompt {
    pub content: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Completion {
    pub stream: bool,
    pub delta: String,
    pub name: String,
    #[serde(rename = "conversationId")]
    pub conversation_id: i64,
    #[serde(rename = "requestId")]
    pub request_id: i64,
    #[serde(rename = "responseId")]
    pub response_id: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    #[serde(rename = "inputTokens")]
    pub input_tokens: usize,
    #[serde(rename = "outputTokens")]
    pub output_tokens: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UsageResponse {
    #[serde(rename = "tokenUsage")]
    pub token_usage: Vec<std::collections::HashMap<String, TokenUsage>>,
    pub dates: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "method")]
pub enum ArrakisResponse {
    Ping {
        id: String,
        payload: Ping,
    },
    Completion {
        id: String,
        payload: Completion,
    },
    CompletionEnd {
        id: String,
        payload: SystemPrompt,
    },
    ConversationList {
        id: String,
        payload: ConversationList,
    },
    Load {
        id: String,
        payload: Conversation,
    },
    Config {
        id: String,
        payload: UserConfig,
    },
    WilliamError {
        id: String,
        payload: WilliamError,
    },
    Preview {
        id: String,
        payload: Preview,
    },
    Usage {
        id: String,
        payload: UsageResponse,
    },
}

// search.rs (for Dewey-related structures)
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DeweyRequest {
    pub k: usize,
    pub query: String,
    pub filters: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeweyResponseItem {
    pub filepath: String,
    pub subset: (u64, u64),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeweyResponse {
    pub results: Vec<DeweyResponseItem>,
}

// config.rs
#[derive(Clone, Debug)]
pub struct RequestParams {
    pub provider: String,
    pub host: String,
    pub path: String,
    pub port: u16,
    pub messages: Vec<Message>,
    pub model: String,
    pub stream: bool,
    pub authorization_token: String,
    pub max_tokens: Option<u16>,
    pub system_prompt: Option<String>,
}
