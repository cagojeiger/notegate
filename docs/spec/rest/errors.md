# REST Error policy

## Error policy

- Missing/invalid auth: `401`
- Authenticated but no active local account: `403`
- Insufficient workspace role: `403`
- Not found or cross-workspace access: `404`
- Invalid field/name/path, malformed limit, or malformed cursor: `400`
- Hash mismatch, root move/delete, duplicate destination, subtree too large: `409`
- Internal errors: `500` with redacted message
