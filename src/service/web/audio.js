// Audio stream state
let activeAudioSession = {
    accountName: null,
    ws: null,
    audioCtx: null,
    micStream: null,
    sampleQueue: [],
    playbackNode: null,
    captureNode: null,
    micSource: null,
    audioElem: null,
    inputDeviceId: null,
    outputDeviceId: null,
    codecRate: 8000
};

// Enumerate available hardware sound cards / audio input and output devices
async function enumerateAudioHardware() {
    try {
        if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices) {
            console.warn("enumerateDevices not supported on this browser.");
            return { inputs: [], outputs: [] };
        }

        let devices = await navigator.mediaDevices.enumerateDevices();
        let inputs = devices.filter(d => d.kind === 'audioinput');
        let outputs = devices.filter(d => d.kind === 'audiooutput');

        // If labels are empty, trigger prompt once so user gives permission
        const needsPermission = (inputs.length > 0 && !inputs[0].label) || (outputs.length > 0 && !outputs[0].label);
        if (needsPermission) {
            try {
                const tempStream = await navigator.mediaDevices.getUserMedia({ audio: true });
                tempStream.getTracks().forEach(t => t.stop());
                devices = await navigator.mediaDevices.enumerateDevices();
                inputs = devices.filter(d => d.kind === 'audioinput');
                outputs = devices.filter(d => d.kind === 'audiooutput');
            } catch (permErr) {
                console.warn("Could not get permission to query audio device labels:", permErr);
            }
        }

        return {
            inputs: inputs.map((d, i) => ({ id: d.deviceId, label: d.label || `Microphone ${i + 1}` })),
            outputs: outputs.map((d, i) => ({ id: d.deviceId, label: d.label || `Speaker/Sound Card ${i + 1}` }))
        };
    } catch (e) {
        console.error("Error enumerating audio devices:", e);
        return { inputs: [], outputs: [] };
    }
}

async function toggleJoinCall(accountName, codecRate, inputDevId, outputDevId) {
    if (activeAudioSession.accountName === accountName) {
        leaveCallAudio();
    } else {
        if (activeAudioSession.accountName) {
            leaveCallAudio();
        }
        await joinCallAudio(accountName, codecRate, inputDevId, outputDevId);
    }
}

async function joinCallAudio(accountName, codecRate = 8000, preferredInputId = null, preferredOutputId = null) {
    try {
        // Find account config audio device defaults if not passed explicitly
        if (!preferredInputId || !preferredOutputId) {
            if (typeof latestStatus !== 'undefined' && latestStatus && latestStatus.accounts) {
                const acc = latestStatus.accounts.find(a => a.name === accountName);
                if (acc) {
                    if (!preferredInputId) preferredInputId = acc.audio_input_device || null;
                    if (!preferredOutputId) preferredOutputId = acc.audio_output_device || null;
                }
            }
        }

        // 1. Get microphone hardware stream with specified deviceId constraint
        let micStream = null;
        let audioConstraint = { audio: true };
        if (preferredInputId) {
            audioConstraint = { audio: { deviceId: { exact: preferredInputId } } };
        }

        try {
            micStream = await navigator.mediaDevices.getUserMedia(audioConstraint);
        } catch (err) {
            console.warn("Could not access specific hardware input sound card, falling back to default mic:", err);
            micStream = await navigator.mediaDevices.getUserMedia({ audio: true });
        }

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

        // Set output sink on AudioContext if supported directly
        if (preferredOutputId && typeof audioCtx.setSinkId === 'function') {
            try {
                await audioCtx.setSinkId(preferredOutputId);
            } catch (e) {
                console.warn("AudioContext.setSinkId failed:", e);
            }
        }

        // 4. Playback node (receiving PCM from WebSocket)
        const bufferSize = 2048;
        const playbackNode = audioCtx.createScriptProcessor(bufferSize, 0, 1);
        playbackNode.onaudioprocess = function(e) {
            const outputBuffer = e.outputBuffer.getChannelData(0);
            for (let i = 0; i < outputBuffer.length; i++) {
                outputBuffer[i] = sampleQueue.shift() || 0.0;
            }
        };

        // Output destination stream setup for sound card selection compatibility
        const mediaStreamDest = audioCtx.createMediaStreamDestination();
        playbackNode.connect(mediaStreamDest);
        playbackNode.connect(audioCtx.destination);

        const audioElem = new Audio();
        audioElem.srcObject = mediaStreamDest.stream;
        audioElem.autoplay = true;
        if (preferredOutputId && typeof audioElem.setSinkId === 'function') {
            try {
                await audioElem.setSinkId(preferredOutputId);
                console.log("Audio output directed to hardware sound card sink:", preferredOutputId);
            } catch (sinkErr) {
                console.warn("Failed to set audio sink ID on element:", sinkErr);
            }
        }
        audioElem.play().catch(e => console.warn("Audio element play error:", e));

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
            micSource,
            audioElem,
            inputDeviceId: preferredInputId,
            outputDeviceId: preferredOutputId,
            codecRate
        };

        document.getElementById('audio-session-account-name').innerText = accountName;
        document.getElementById('audio-session-banner').style.display = 'flex';
        
        // Populate live sound card controls in banner
        await updateBannerAudioDevices();
        updateDashboard();

    } catch (err) {
        console.error("Failed to join call audio:", err);
        showNotification("Could not access microphone or connect to audio service: " + err.message, "error");
    }
}

async function updateBannerAudioDevices() {
    const hw = await enumerateAudioHardware();
    const bannerInputSelect = document.getElementById('banner-audio-input');
    const bannerOutputSelect = document.getElementById('banner-audio-output');

    if (bannerInputSelect) {
        bannerInputSelect.innerHTML = '<option value="">Default Hardware Input</option>';
        hw.inputs.forEach(d => {
            const opt = document.createElement('option');
            opt.value = d.id;
            opt.innerText = d.label;
            if (d.id === activeAudioSession.inputDeviceId) opt.selected = true;
            bannerInputSelect.appendChild(opt);
        });
    }

    if (bannerOutputSelect) {
        bannerOutputSelect.innerHTML = '<option value="">Default Hardware Output</option>';
        hw.outputs.forEach(d => {
            const opt = document.createElement('option');
            opt.value = d.id;
            opt.innerText = d.label;
            if (d.id === activeAudioSession.outputDeviceId) opt.selected = true;
            bannerOutputSelect.appendChild(opt);
        });
    }
}

async function changeActiveAudioInput(newDeviceId) {
    if (!activeAudioSession.accountName) return;
    try {
        if (activeAudioSession.micStream) {
            activeAudioSession.micStream.getTracks().forEach(track => track.stop());
        }
        if (activeAudioSession.micSource) {
            activeAudioSession.micSource.disconnect();
        }

        const audioConstraint = newDeviceId ? { audio: { deviceId: { exact: newDeviceId } } } : { audio: true };
        const newMicStream = await navigator.mediaDevices.getUserMedia(audioConstraint);
        const newMicSource = activeAudioSession.audioCtx.createMediaStreamSource(newMicStream);
        newMicSource.connect(activeAudioSession.captureNode);

        activeAudioSession.micStream = newMicStream;
        activeAudioSession.micSource = newMicSource;
        activeAudioSession.inputDeviceId = newDeviceId;
        showNotification("Hardware sound card input updated", "info");
    } catch (e) {
        console.error("Failed to switch audio input device:", e);
        showNotification("Could not switch microphone hardware device", "error");
    }
}

async function changeActiveAudioOutput(newDeviceId) {
    if (!activeAudioSession.accountName) return;
    try {
        if (activeAudioSession.audioElem && typeof activeAudioSession.audioElem.setSinkId === 'function') {
            await activeAudioSession.audioElem.setSinkId(newDeviceId || "");
        }
        if (activeAudioSession.audioCtx && typeof activeAudioSession.audioCtx.setSinkId === 'function') {
            await activeAudioSession.audioCtx.setSinkId(newDeviceId || "");
        }
        activeAudioSession.outputDeviceId = newDeviceId;
        showNotification("Hardware sound card output updated", "info");
    } catch (e) {
        console.error("Failed to switch audio output device:", e);
        showNotification("Could not switch sound card output device", "error");
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

    if (activeAudioSession.audioElem) {
        activeAudioSession.audioElem.pause();
        activeAudioSession.audioElem.srcObject = null;
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
        micSource: null,
        audioElem: null,
        inputDeviceId: null,
        outputDeviceId: null,
        codecRate: 8000
    };

    document.getElementById('audio-session-banner').style.display = 'none';
    updateDashboard();
}
