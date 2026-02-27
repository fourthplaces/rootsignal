use ai_client::openai::OpenAi;
use ai_client::traits::EmbedAgent;
use anyhow::Result;

// TextEmbedder trait is now defined in rootsignal-common.
pub use rootsignal_common::TextEmbedder;

/// Wrapper around Voyage AI embeddings via the OpenAI-compatible API.
pub struct Embedder {
    client: OpenAi,
}

impl Embedder {
    /// Create a new embedder using Voyage AI's API.
    pub fn new(voyage_api_key: &str) -> Self {
        let client = OpenAi::new(voyage_api_key, "voyage-3-large")
            .with_base_url("https://api.voyageai.com/v1")
            .with_embedding_model("voyage-3-large");
        Self { client }
    }

    /// Embed a single text. Returns a 1024-dim vector.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.client.embed(text.to_string()).await
    }

    /// Embed multiple texts in a batch.
    pub async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        self.client.embed_batch(texts).await
    }
}

#[async_trait::async_trait]
impl TextEmbedder for Embedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.client.embed(text.to_string()).await
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        self.client.embed_batch(texts).await
    }
}

/// No-op embedder for contexts that don't need embeddings (e.g. API source creation).
pub struct NoOpEmbedder;

#[async_trait::async_trait]
impl TextEmbedder for NoOpEmbedder {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![])
    }

    async fn embed_batch(&self, _texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Ok(vec![])
    }
}
