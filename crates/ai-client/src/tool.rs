use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::error::Error;

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[async_trait]
pub trait Tool: Send + Sync {
    const NAME: &'static str;
    type Error: Error + Send + Sync + 'static;
    type Args: DeserializeOwned + Send + Sync;
    type Output: Serialize + Send + Sync;

    async fn definition(&self) -> ToolDefinition;
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>;
}

#[async_trait]
pub trait DynTool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn definition(&self) -> ToolDefinition;
    async fn call_json(&self, args: Value) -> Result<Value, String>;
}

pub struct ToolWrapper<T: Tool>(pub T);

#[async_trait]
impl<T: Tool> DynTool for ToolWrapper<T> {
    fn name(&self) -> &'static str {
        T::NAME
    }

    async fn definition(&self) -> ToolDefinition {
        self.0.definition().await
    }

    async fn call_json(&self, args: Value) -> Result<Value, String> {
        let parsed_args: T::Args =
            serde_json::from_value(args).map_err(|e| format!("Failed to parse args: {}", e))?;

        let result = self
            .0
            .call(parsed_args)
            .await
            .map_err(|e| format!("Tool error: {}", e))?;

        serde_json::to_value(result).map_err(|e| format!("Failed to serialize result: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct EchoArgs {
        message: String,
    }

    #[derive(Debug)]
    struct EchoError;
    impl std::fmt::Display for EchoError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "echo error")
        }
    }
    impl std::error::Error for EchoError {}

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        const NAME: &'static str = "echo";
        type Error = EchoError;
        type Args = EchoArgs;
        type Output = String;

        async fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: Self::NAME.to_string(),
                description: "Echo back the input".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    },
                    "required": ["message"]
                }),
            }
        }

        async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
            Ok(args.message)
        }
    }

    #[tokio::test]
    async fn test_tool_wrapper() {
        let tool: Box<dyn DynTool> = Box::new(ToolWrapper(EchoTool));
        assert_eq!(tool.name(), "echo");

        let result = tool
            .call_json(serde_json::json!({"message": "hello"}))
            .await
            .unwrap();
        assert_eq!(result, serde_json::json!("hello"));
    }
}
