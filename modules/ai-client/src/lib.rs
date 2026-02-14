pub mod claude;
pub mod error;
pub mod openai;
pub mod openrouter;
pub mod tool;
pub mod traits;
pub mod util;

pub use claude::Claude;
pub use error::AiError;
pub use openai::OpenAi;
pub use openrouter::OpenRouter;
pub use tool::{DynTool, Tool, ToolDefinition, ToolWrapper};
pub use traits::{Agent, EmbedAgent, Message, MessageRole, OutputBuilder, PromptBuilder};
pub use util::{strip_code_blocks, truncate_to_char_boundary};
