use ai_client::openai::OpenAi;
use ai_client::traits::EmbedAgent;
use anyhow::Result;

// --- TextEmbedder trait ---

#[async_trait::async_trait]
pub trait TextEmbedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}

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
