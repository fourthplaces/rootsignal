/// Implement Restate SDK serialization traits for types that already have serde derives.
#[macro_export]
macro_rules! impl_restate_serde {
    ($type:ty) => {
        impl restate_sdk::serde::Serialize for $type {
            type Error = serde_json::Error;

            fn serialize(&self) -> Result<bytes::Bytes, Self::Error> {
                serde_json::to_vec(self).map(bytes::Bytes::from)
            }
        }

        impl restate_sdk::serde::Deserialize for $type {
            type Error = serde_json::Error;

            fn deserialize(bytes: &mut bytes::Bytes) -> Result<Self, Self::Error> {
                serde_json::from_slice(bytes)
            }
        }

        impl restate_sdk::serde::WithContentType for $type {
            fn content_type() -> &'static str {
                "application/json"
            }
        }
    };
}
