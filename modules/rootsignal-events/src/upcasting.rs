//! Event upcasting — transforms old event payloads to the current schema.
//!
//! Called before deserialization so that old events stored with `schema_v < CURRENT`
//! can be transparently upgraded. Each upcaster matches on `(event_type, schema_v)`
//! and mutates the JSON payload in place.
//!
//! No upcasters exist yet — this module is the hook point for future schema evolution.

/// Transform an event payload from an older schema version to the current one.
///
/// Mutates `payload` in place. If no upcaster matches, the payload is left unchanged.
pub fn upcast(_event_type: &str, _schema_v: i16, _payload: &mut serde_json::Value) {
    // Future upcasters go here. Example:
    //
    // match (event_type, schema_v) {
    //     ("gathering_created", 1) => {
    //         // v1 → v2: rename "location" to "venue"
    //         if let Some(obj) = payload.as_object_mut() {
    //             if let Some(loc) = obj.remove("location") {
    //                 obj.insert("venue".to_string(), loc);
    //             }
    //         }
    //     }
    //     _ => {}
    // }
}
