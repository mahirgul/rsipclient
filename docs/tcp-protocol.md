# TCP Control Protocol

The service listens on `127.0.0.1:5090` by default. Communication is line-delimited JSON.

## Request format

```json
{"cmd":"<command>","account":"<name>","target":"<value>"}
```

| Field | Required | Description |
|-------|----------|-------------|
| `cmd` | Yes | Command name |
| `account` | Depends | Account name from config |
| `target` | Depends | Target URI, file path, etc. |

## Commands

### `register`

Register an account with the SIP server.

```json
{"cmd":"register","account":"alice"}
```

### `call`

Place an outbound call.

```json
{"cmd":"call","account":"alice","target":"sip:bob@sip.example.com"}
```

### `hangup`

End the current call.

```json
{"cmd":"hangup","account":"alice"}
```

### `cancel`

Cancel a pending INVITE.

```json
{"cmd":"cancel","account":"alice"}
```

### `play`

Play a WAV file to the remote party (must be in a call).

```json
{"cmd":"play","account":"alice","target":"audio/message.wav"}
```

### `status`

List all accounts and their current state.

```json
{"cmd":"status"}
```

### `shutdown`

Gracefully stop the service.

```json
{"cmd":"shutdown"}
```

## Response format

```json
{"ok":true,"msg":"..."}
{"ok":false,"msg":"error description"}
```

## Examples (netcat)

```bash
echo '{"cmd":"status"}' | nc 127.0.0.1 5090
echo '{"cmd":"register","account":"alice"}' | nc 127.0.0.1 5090
echo '{"cmd":"call","account":"alice","target":"sip:bob@example.com"}' | nc 127.0.0.1 5090
echo '{"cmd":"shutdown"}' | nc 127.0.0.1 5090
```
