-- ====================================================================
-- Sample Lua 5.4 Script Plugin for rsipclient
-- ====================================================================
-- Lua is a lightweight, widely-used embedded scripting language.
--
-- Global tables provided:
-- - event: Table containing event details (incoming_call, dtmf, etc.)
-- - context: Table containing caller & session details during IVR steps
-- - rsip.log(level, message): Function to log messages to rsipclient.

local caller = (context and context.caller) or "Unknown"

rsip.log("info", "[Lua Script] Processing call from: " .. caller)

-- Example IVR decision logic
if string.find(caller, "100") then
    return {
        action = "transfer",
        target = "sip:vip@example.com"
    }
else
    return {
        action = "playback",
        target = "welcome.wav"
    }
end
