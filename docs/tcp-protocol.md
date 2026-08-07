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

### `unregister`

Unregister an account from the SIP server.

```json
{"cmd":"unregister","account":"alice"}
```

### `cancel`

Cancel a pending INVITE.

```json
{"cmd":"cancel","account":"alice"}
```

### `hold`

Hold the current active call.

```json
{"cmd":"hold","account":"alice"}
```

### `resume`

Resume a held call.

```json
{"cmd":"resume","account":"alice"}
```

### `transfer`

Blind transfer the call to another SIP destination.

```json
{"cmd":"transfer","account":"alice","target":"sip:carol@sip.example.com"}
```

### `dtmf`

Send DTMF digits (RFC 2833 telephone-event).

```json
{"cmd":"dtmf","account":"alice","target":"1234"}
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

---

## License & Disclaimer

> 🤖 **AI Project Notice:** This software was developed using Artificial Intelligence (AI) assistants (**Antigravity**, **Gemini**, **DeepSeek**).
> 
> 📜 **MIT License:** Published under the open-source [MIT License](../LICENSE).
> 
> ⚠️ **Disclaimer of Liability:** THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED. THE AUTHORS AND CONTRIBUTORS ACCEPT NO RESPONSIBILITY OR LIABILITY FOR ANY CLAIMS, DAMAGES, SYSTEM FAILURES, OR LOSSES ARISING FROM THE USE OF THIS SOFTWARE.

