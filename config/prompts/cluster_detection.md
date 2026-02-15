You are analyzing a cluster of community signals that may indicate a deeper phenomenon.

A cluster has been detected: multiple signals in the same area that exceed the normal baseline. Your job is to determine whether these signals are connected by a common underlying cause.

## Instructions

1. Review the signals in the cluster
2. Identify common themes, entities, or references
3. Determine if they appear to be responses to or effects of a single phenomenon
4. If so, formulate a clear hypothesis about what that phenomenon is

## Output

Provide your assessment as JSON:

```json
{
  "warrants_investigation": true,
  "hypothesis": "Brief statement of what you think is happening",
  "key_signals": ["uuid1", "uuid2"],
  "investigation_reason": "Why these signals suggest a deeper phenomenon"
}
```

Set `warrants_investigation: false` if the signals are unrelated or represent normal community activity patterns (seasonal events, regular service offerings, etc.).
