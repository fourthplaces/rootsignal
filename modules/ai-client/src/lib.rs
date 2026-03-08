// Model ID constants — single source of truth
pub mod models {
    // Anthropic
    pub const SONNET_4_6: &str = "claude-sonnet-4-6";
    pub const SONNET_4_5: &str = "claude-sonnet-4-5-20250929";
    pub const SONNET_4: &str = "claude-sonnet-4-20250514";
    pub const HAIKU_4_5: &str = "claude-haiku-4-5-20251001";

    // Google
    pub const GEMINI_3_FLASH: &str = "gemini-3-flash-preview";
    pub const GEMINI_2_5_FLASH: &str = "gemini-2.5-flash";

    // OpenAI
    pub const GPT_5_MINI: &str = "gpt-5-mini";
    pub const GPT_4_1_MINI: &str = "gpt-4.1-mini";
}

pub mod circuit_breaker;
pub mod claude;
pub mod error;
pub mod fallback;
pub mod gemini;
pub mod openai;
pub mod openrouter;
pub mod tool;
pub mod traits;
pub mod util;

pub use circuit_breaker::CircuitBreaker;
pub use claude::Claude;
pub use error::AiError;
pub use fallback::FallbackAgent;
pub use gemini::Gemini;
pub use openai::OpenAi;
pub use openrouter::OpenRouter;
pub use tool::{DynTool, Tool, ToolDefinition, ToolWrapper};
pub use traits::{ai_extract, Agent, EmbedAgent, Message, MessageRole, OutputBuilder, PromptBuilder};
pub use util::{strip_code_blocks, truncate_to_char_boundary};
