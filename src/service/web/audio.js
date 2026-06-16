// Audio stream state
let activeAudioSession = {
    accountName: null,
    ws: null,
    audioCtx: null,
    micStream: null,
    sampleQueue: [],
    playbackNode: null,
    captureNode: null,
    micSource: null
};

async function toggleJoinCall(accountName, codecRate) {
    if (activeAudioSession.accountName === accountName) {
        leaveCallAudio();
    } else {
        if (activeAudioSession.accountName) {
            leaveCallAudio();
        }
        await joinCallAudio(accountName, codecRate);
    }
}

async function joinCallAudio(accountName, codecRate) {
    try {
        // 1. Get microphone permission
        const micStream = await navigator.mediaDevices.getUserMedia({ audio: true });
        
        // 2. Open WebSocket connection
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const host = window.location.host;
        const token = getToken();
        const wsUrl = `${protocol}//${host}/api/accounts/${accountName}/audio-ws?token=${token}`;
        
        const ws = new WebSocket(wsUrl);
        ws.binaryType = 'arraybuffer';

        // 3. Setup AudioContext
        const audioCtx = new (window.AudioContext || window.webkitAudioContext)({ sampleRate: codecRate });
        const sampleQueue = [];

        // 4. Playback node (receiving PCM from WebSocket)
        const bufferSize = 2048;
        const playbackNode = audioCtx.createScriptProcessor(bufferSize, 0, 1);
        playbackNode.onaudioprocess = function(e) {
            const outputBuffer = e.outputBuffer.getChannelData(0);
            for (let i = 0; i < outputBuffer.length; i++) {
                outputBuffer[i] = sampleQueue.shift() || 0.0;
            }
        };
        playbackNode.connect(audioCtx.destination);

        // 5. Capture node (sending microphone audio over WS)
        const micSource = audioCtx.createMediaStreamSource(micStream);
        const captureNode = audioCtx.createScriptProcessor(bufferSize, 1, 1);
        captureNode.onaudioprocess = function(e) {
            const inputBuffer = e.inputBuffer.getChannelData(0);
            const i16Samples = new Int16Array(inputBuffer.length);
            for (let i = 0; i < inputBuffer.length; i++) {
                i16Samples[i] = Math.max(-32768, Math.min(32767, inputBuffer[i] * 32768));
            }
            if (ws.readyState === WebSocket.OPEN) {
                ws.send(i16Samples.buffer);
            }
        };
        micSource.connect(captureNode);
        captureNode.connect(audioCtx.destination);

        ws.onmessage = function(event) {
            const intData = new Int16Array(event.data);
            for (let i = 0; i < intData.length; i++) {
                sampleQueue.push(intData[i] / 32768.0);
            }
            if (sampleQueue.length > codecRate * 1.5) {
                sampleQueue.splice(0, sampleQueue.length - codecRate);
            }
        };

        ws.onclose = function() {
            console.log("Audio WebSocket closed.");
            if (activeAudioSession.accountName === accountName) {
                leaveCallAudio();
            }
        };

        ws.onerror = function(err) {
            console.error("Audio WebSocket error:", err);
            showNotification("Audio connection encountered an error.", "error");
        };

        activeAudioSession = {
            accountName,
            ws,
            audioCtx,
            micStream,
            sampleQueue,
            playbackNode,
            captureNode,
            micSource
        };

        document.getElementById('audio-session-account-name').innerText = accountName;
        document.getElementById('audio-session-banner').style.display = 'flex';
        
        updateDashboard();

    } catch (err) {
        console.error("Failed to join call audio:", err);
        showNotification("Could not access microphone or connect to audio service: " + err.message, "error");
    }
}

function leaveCallAudio() {
    if (!activeAudioSession.accountName) return;

    console.log("Leaving call audio session for:", activeAudioSession.accountName);

    if (activeAudioSession.ws) {
        activeAudioSession.ws.close();
    }

    if (activeAudioSession.micStream) {
        activeAudioSession.micStream.getTracks().forEach(track => track.stop());
    }

    if (activeAudioSession.micSource && activeAudioSession.captureNode) {
        activeAudioSession.micSource.disconnect();
        activeAudioSession.captureNode.disconnect();
    }
    if (activeAudioSession.playbackNode) {
        activeAudioSession.playbackNode.disconnect();
    }

    if (activeAudioSession.audioCtx) {
        activeAudioSession.audioCtx.close();
    }

    activeAudioSession = {
        accountName: null,
        ws: null,
        audioCtx: null,
        micStream: null,
        sampleQueue: [],
        playbackNode: null,
        captureNode: null,
        micSource: null
    };

    document.getElementById('audio-session-banner').style.display = 'none';
    updateDashboard();
}
