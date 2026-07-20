// ====================================================================
// Sample Rhai Script Plugin for rsipclient
// ====================================================================
// Rhai is a pure Rust embedded scripting engine with JavaScript-like syntax.
//
// Exposed variables:
// - event: Object containing event details (incoming_call, dtmf, etc.)
// - context: Object containing caller & session details during IVR steps
//
// Exposed functions:
// - rsip_log(level, message): Logs a message to the rsipclient logger.

let caller = if context.contains("caller") { context.caller } else { "Unknown" };

rsip_log("info", `[Rhai Script] Processing call from: ${caller}`);

// Example IVR decision: If caller contains '100', transfer to VIP extension
if caller.contains("100") {
    #{
        action: "transfer",
        target: "sip:vip@example.com"
    }
} else {
    // Default action: Play welcome message
    #{
        action: "playback",
        target: "welcome.wav"
    }
}
