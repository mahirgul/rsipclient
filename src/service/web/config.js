// Trigger Auto Answer IVR subfields visibility
document.getElementById('acc-auto-answer').addEventListener('change', (e) => {
    const ivrFields = document.getElementById('ivr-subfields');
    ivrFields.style.display = e.target.checked ? 'block' : 'none';
});

// Cache for WAV files to use in dropdowns
let cachedAudioFiles = [];

async function fetchAudioFiles() {
    try {
        const res = await fetch(`${API_URL}/api/audio`, { headers: getAuthHeaders() });
        if (res.ok) {
            cachedAudioFiles = await res.json();
        }
    } catch (err) {
        console.error("Failed to fetch audio files for dropdown:", err);
    }
}

function populateIvrDropdowns(selectedWelcome, menuMap = {}) {
    const welcomeSelect = document.getElementById('acc-ivr-welcome');
    welcomeSelect.innerHTML = '<option value="">-- Select Welcome Audio --</option>';
    
    cachedAudioFiles.forEach(file => {
        const opt = document.createElement('option');
        opt.value = file.name;
        opt.innerText = `${file.name} (${file.duration_secs.toFixed(1)}s)`;
        welcomeSelect.appendChild(opt);
    });
    
    if (selectedWelcome) {
        welcomeSelect.value = selectedWelcome;
    }

    // Populate DTMF rows
    const listContainer = document.getElementById('ivr-menu-builder-list');
    listContainer.innerHTML = '';
    
    const digits = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "*", "#"];
    digits.forEach(digit => {
        const mappedVal = menuMap[digit] || "";
        const isMapped = mappedVal.length > 0;
        
        let action = "playback";
        let param = "";
        
        if (isMapped) {
            if (mappedVal.startsWith("transfer:")) {
                action = "transfer";
                param = mappedVal.substring(9);
            } else if (mappedVal.startsWith("playback:")) {
                action = "playback";
                param = mappedVal.substring(9);
            } else if (mappedVal.startsWith("record:")) {
                action = "record";
                param = mappedVal.substring(7); // format: filename.wav:duration
            } else if (mappedVal === "hold") {
                action = "hold";
            } else if (mappedVal === "hangup") {
                action = "hangup";
            }
        }
        
        const row = document.createElement('div');
        row.className = 'ivr-menu-row';
        row.setAttribute('data-digit', digit);
        
        row.innerHTML = `
            <input type="checkbox" class="ivr-row-enable" ${isMapped ? 'checked' : ''} style="margin-right: 0.5rem; cursor: pointer;">
            <span class="ivr-digit-badge">${digit}</span>
            <select class="form-control ivr-row-action" style="flex: 1; min-width: 100px; padding: 0.3rem; margin: 0 0.5rem; background: rgba(31, 41, 55, 0.9); font-size: 0.8rem; border-radius: 4px;">
                <option value="playback" ${action === 'playback' ? 'selected' : ''}>Playback</option>
                <option value="transfer" ${action === 'transfer' ? 'selected' : ''}>Transfer</option>
                <option value="record" ${action === 'record' ? 'selected' : ''}>Record</option>
                <option value="hold" ${action === 'hold' ? 'selected' : ''}>Hold</option>
                <option value="hangup" ${action === 'hangup' ? 'selected' : ''}>Hangup</option>
            </select>
            <div class="ivr-param-container" style="flex: 2; display: flex; align-items: center;">
                <!-- Dynamic param control based on action -->
            </div>
        `;
        
        listContainer.appendChild(row);
        
        const actionSelect = row.querySelector('.ivr-row-action');
        const paramContainer = row.querySelector('.ivr-param-container');
        
        const updateParamFields = () => {
            const act = actionSelect.value;
            paramContainer.innerHTML = '';
            
            if (act === 'playback') {
                const sel = document.createElement('select');
                sel.className = 'form-control ivr-row-param';
                sel.style.padding = '0.3rem';
                sel.style.fontSize = '0.8rem';
                sel.style.background = 'rgba(31, 41, 55, 0.9)';
                sel.innerHTML = '<option value="">-- Select WAV --</option>';
                cachedAudioFiles.forEach(f => {
                    const opt = document.createElement('option');
                    opt.value = f.name;
                    opt.innerText = f.name;
                    sel.appendChild(opt);
                });
                if (action === 'playback' && param) {
                    sel.value = param;
                }
                paramContainer.appendChild(sel);
            } else if (act === 'transfer') {
                const inp = document.createElement('input');
                inp.type = 'text';
                inp.className = 'form-control ivr-row-param';
                inp.placeholder = 'sip:100@domain';
                inp.style.padding = '0.3rem';
                inp.style.fontSize = '0.8rem';
                if (action === 'transfer' && param) {
                    inp.value = param;
                }
                paramContainer.appendChild(inp);
            } else if (act === 'record') {
                const container = document.createElement('div');
                container.style.display = 'flex';
                container.style.gap = '0.3rem';
                container.style.width = '100%';
                
                let recFile = 'voicemail.wav';
                let recDur = '30';
                if (action === 'record' && param) {
                    const parts = param.split(':');
                    recFile = parts[0] || 'voicemail.wav';
                    recDur = parts[1] || '30';
                }
                
                container.innerHTML = `
                    <input type="text" class="form-control ivr-rec-file" placeholder="voicemail.wav" value="${recFile}" style="flex: 2; padding: 0.3rem; font-size: 0.8rem;">
                    <input type="number" class="form-control ivr-rec-dur" placeholder="30" value="${recDur}" style="flex: 1; padding: 0.3rem; font-size: 0.8rem;" min="5" max="300">
                `;
                paramContainer.appendChild(container);
            } else {
                paramContainer.innerHTML = '<span style="font-size: 0.75rem; opacity: 0.5;">No parameters needed</span>';
            }
        };
        
        actionSelect.addEventListener('change', updateParamFields);
        updateParamFields();
    });
}

// Load account settings configurations
async function loadAccountsConfig() {
    try {
        const res = await fetch(`${API_URL}/api/accounts`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        const accounts = await res.json();

        const configBody = document.getElementById('accounts-config-body');
        configBody.innerHTML = '';

        if (accounts.length === 0) {
            configBody.innerHTML = `<tr><td colspan="9" style="text-align: center; color: var(--text-secondary);">No accounts found. Create a new one.</td></tr>`;
        } else {
            accounts.forEach(acc => {
                const autoAns = acc.auto_answer ? 'Yes (IVR)' : 'No';
                const soundCards = (acc.audio_input_device || acc.audio_output_device)
                    ? `🎙️ ${acc.audio_input_device ? 'Custom Mic' : 'Default'} / 🔊 ${acc.audio_output_device ? 'Custom Output' : 'Default'}`
                    : 'System Default';
                const tr = document.createElement('tr');
                tr.innerHTML = `
                    <td style="font-weight:600;">${acc.name}</td>
                    <td>${acc.username}</td>
                    <td>${acc.server}</td>
                    <td style="text-transform: uppercase;">${acc.codec || 'pcmu'}</td>
                    <td>${acc.sip_port === 0 ? 'Auto' : acc.sip_port}</td>
                    <td>${acc.rtp_port_start}-${acc.rtp_port_end}</td>
                    <td>${autoAns}</td>
                    <td style="font-size:0.8rem; color: var(--text-secondary);">${soundCards}</td>
                    <td class="action-group">
                        <button class="action-btn" title="Edit account" onclick="openEditAccountModal('${acc.name}')">✎</button>
                        <button class="action-btn" title="Delete account" style="color:var(--accent-error);" onclick="deleteAccount('${acc.name}')">🗑</button>
                    </td>
                `;
                configBody.appendChild(tr);
            });
        }
    } catch (err) {
        console.error("Failed to load accounts:", err);
    }
}

async function loadGlobalSettings() {
    const token = getToken();
    if (!token) return;

    try {
        const res = await fetch(`${API_URL}/api/config`, {
            headers: { 'Authorization': `Bearer ${token}` }
        });
        if (res.status === 401) return logout();
        if (!res.ok) throw new Error("Load failed");

        const config = await res.json();

        // Fill form fields
        if (config.web) {
            document.getElementById('settings-web-port').value = config.web.port || 9090;
            document.getElementById('settings-web-user').value = config.web.username || 'admin';
            document.getElementById('settings-web-pass').value = config.web.password || 'admin';
        } else {
            document.getElementById('settings-web-port').value = 9090;
            document.getElementById('settings-web-user').value = 'admin';
            document.getElementById('settings-web-pass').value = 'admin';
        }

        if (config.commands_api) {
            document.getElementById('settings-cmd-port').value = config.commands_api.port || 9099;
            document.getElementById('settings-cmd-user').value = config.commands_api.username || '';
            document.getElementById('settings-cmd-pass').value = config.commands_api.password || '';
        } else {
            document.getElementById('settings-cmd-port').value = 9099;
            document.getElementById('settings-cmd-user').value = '';
            document.getElementById('settings-cmd-pass').value = '';
        }

        if (config.syslog) {
            document.getElementById('settings-syslog-enabled').checked = config.syslog.enabled !== false;
            document.getElementById('settings-syslog-server').value = config.syslog.server || '127.0.0.1:514';
            document.getElementById('settings-syslog-protocol').value = config.syslog.protocol || 'udp';
            document.getElementById('settings-syslog-facility').value = config.syslog.facility || 'user';
            document.getElementById('settings-syslog-app-name').value = config.syslog.app_name || 'rsipclient';
        } else {
            document.getElementById('settings-syslog-enabled').checked = false;
            document.getElementById('settings-syslog-server').value = '127.0.0.1:514';
            document.getElementById('settings-syslog-protocol').value = 'udp';
            document.getElementById('settings-syslog-facility').value = 'user';
            document.getElementById('settings-syslog-app-name').value = 'rsipclient';
        }

        // Fill raw config text area
        document.getElementById('settings-raw-config').value = JSON.stringify(config, null, 4);
    } catch (err) {
        console.error("Failed to load settings:", err);
    }
}

async function saveGlobalSettings() {
    const token = getToken();
    if (!token) return;

    try {
        let updatedConfig = null;
        const rawText = document.getElementById('settings-raw-config').value;

        try {
            updatedConfig = JSON.parse(rawText);
        } catch (e) {
            showNotification("Invalid JSON format in the raw editor!", "error");
            return;
        }

        // Also sync basic form values into updatedConfig in case they edited form fields
        if (!updatedConfig.web) updatedConfig.web = {};
        updatedConfig.web.port = parseInt(document.getElementById('settings-web-port').value) || 9090;
        updatedConfig.web.username = document.getElementById('settings-web-user').value || 'admin';
        updatedConfig.web.password = document.getElementById('settings-web-pass').value || 'admin';

        const cmdPort = parseInt(document.getElementById('settings-cmd-port').value);
        const cmdUser = document.getElementById('settings-cmd-user').value;
        const cmdPass = document.getElementById('settings-cmd-pass').value;

        if (cmdPort) {
            if (!updatedConfig.commands_api) updatedConfig.commands_api = {};
            updatedConfig.commands_api.port = cmdPort;
            updatedConfig.commands_api.username = cmdUser ? cmdUser : null;
            updatedConfig.commands_api.password = cmdPass ? cmdPass : null;
        } else {
            updatedConfig.commands_api = null;
        }

        const syslogEnabled = document.getElementById('settings-syslog-enabled').checked;
        const syslogServer = document.getElementById('settings-syslog-server').value || '127.0.0.1:514';
        const syslogProtocol = document.getElementById('settings-syslog-protocol').value || 'udp';
        const syslogFacility = document.getElementById('settings-syslog-facility').value || 'user';
        const syslogAppName = document.getElementById('settings-syslog-app-name').value || 'rsipclient';

        updatedConfig.syslog = {
            enabled: syslogEnabled,
            server: syslogServer,
            protocol: syslogProtocol,
            facility: syslogFacility,
            hostname: (updatedConfig.syslog && updatedConfig.syslog.hostname) ? updatedConfig.syslog.hostname : null,
            app_name: syslogAppName
        };

        document.getElementById('settings-raw-config').value = JSON.stringify(updatedConfig, null, 4);

        const res = await fetch(`${API_URL}/api/config`, {
            method: 'PUT',
            headers: {
                'Authorization': `Bearer ${token}`,
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(updatedConfig)
        });

        if (res.status === 401) return logout();
        const data = await res.json();
        if (data.success) {
            showNotification("Settings updated and service clients reloaded successfully!", "success");
            loadGlobalSettings();
        } else {
            showNotification("Failed to update settings: " + (data.msg || "Unknown error"), "error");
        }
    } catch (err) {
        showNotification("Failed to save settings: " + err, "error");
    }
}

// Trigger manual registration API calls
async function registerAccount(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/register`, {
            method: 'POST',
            headers: getAuthHeaders()
        });
        const data = await res.json();
        if (data.success) {
            showNotification(data.msg, "success");
        } else {
            showNotification(data.msg, "error");
        }
        updateDashboard();
    } catch (err) {
        showNotification("Failed to send register command", "error");
    }
}

async function unregisterAccount(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/unregister`, {
            method: 'POST',
            headers: getAuthHeaders()
        });
        const data = await res.json();
        if (data.success) {
            showNotification(data.msg, "success");
        } else {
            showNotification(data.msg, "error");
        }
        updateDashboard();
    } catch (err) {
        showNotification("Failed to send unregister command", "error");
    }
}

async function populateModalAudioDevices(selectedInputId = "", selectedOutputId = "") {
    const hw = await enumerateAudioHardware();
    const inputSelect = document.getElementById('acc-audio-input');
    const outputSelect = document.getElementById('acc-audio-output');

    if (inputSelect) {
        inputSelect.innerHTML = '<option value="">System Default Input Device</option>';
        hw.inputs.forEach(d => {
            const opt = document.createElement('option');
            opt.value = d.id;
            opt.innerText = d.label;
            if (d.id === selectedInputId) opt.selected = true;
            inputSelect.appendChild(opt);
        });
    }

    if (outputSelect) {
        outputSelect.innerHTML = '<option value="">System Default Output Device</option>';
        hw.outputs.forEach(d => {
            const opt = document.createElement('option');
            opt.value = d.id;
            opt.innerText = d.label;
            if (d.id === selectedOutputId) opt.selected = true;
            outputSelect.appendChild(opt);
        });
    }
}

// Helper to toggle secret input visibility without triggering browser password managers
function toggleSecretVisibility(inputId, btn) {
    const el = document.getElementById(inputId);
    if (!el) return;
    if (el.classList.contains('revealed')) {
        el.classList.remove('revealed');
        if (btn) btn.textContent = '👁️';
    } else {
        el.classList.add('revealed');
        if (btn) btn.textContent = '🙈';
    }
}

// Account addition and modification forms
async function openAddAccountModal() {
    document.getElementById('account-form').reset();
    document.getElementById('acc-username').value = '';
    document.getElementById('acc-password').value = '';
    document.getElementById('acc-password').classList.remove('revealed');
    const pwdToggle = document.getElementById('acc-password-toggle');
    if (pwdToggle) pwdToggle.textContent = '👁️';

    document.getElementById('edit-original-name').value = '';
    document.getElementById('modal-mode-title').innerText = 'Add SIP Account';
    document.getElementById('acc-name').disabled = false;
    document.getElementById('ivr-subfields').style.display = 'none';

    // Reset advanced options to default values
    document.getElementById('acc-ivr-timeout').value = 10;
    document.getElementById('acc-ivr-default').value = '';
    document.getElementById('acc-display-name').value = '';
    document.getElementById('acc-user-agent').value = '';
    document.getElementById('acc-register-expiry').value = 3600;
    document.getElementById('acc-register-retry').value = 30;
    document.getElementById('acc-proxy').value = '';
    document.getElementById('acc-early-media').checked = true;
    document.getElementById('acc-session-timers').checked = false;

    // Load WAVs and render builder empty
    await fetchAudioFiles();
    populateIvrDropdowns("", {});

    // Populate sound cards
    await populateModalAudioDevices("", "");

    document.getElementById('account-modal').classList.add('active');
}

async function openEditAccountModal(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts`, { headers: getAuthHeaders() });
        const accounts = await res.json();
        const acc = accounts.find(a => a.name === name);
        if (!acc) return;

        document.getElementById('edit-original-name').value = acc.name;
        document.getElementById('acc-name').value = acc.name;
        document.getElementById('acc-name').disabled = true; // Cannot rename ID during edit
        document.getElementById('acc-username').value = acc.username;
        document.getElementById('acc-password').value = acc.password;
        document.getElementById('acc-password').classList.remove('revealed');
        const pwdToggle = document.getElementById('acc-password-toggle');
        if (pwdToggle) pwdToggle.textContent = '👁️';
        document.getElementById('acc-server').value = acc.server;
        document.getElementById('acc-domain').value = acc.domain || '';
        document.getElementById('acc-sip-port').value = acc.sip_port;
        document.getElementById('acc-codec').value = acc.codec || 'pcmu';
        document.getElementById('acc-rtp-start').value = acc.rtp_port_start;
        document.getElementById('acc-rtp-end').value = acc.rtp_port_end;
        document.getElementById('acc-transport').value = acc.transport || 'udp';
        document.getElementById('acc-auth-method').value = acc.auth_method || 'md5';
        document.getElementById('acc-auto-answer').checked = acc.auto_answer || false;

        await fetchAudioFiles();

        const ivrFields = document.getElementById('ivr-subfields');
        if (acc.auto_answer) {
            ivrFields.style.display = 'block';
            document.getElementById('acc-ivr-timeout').value = acc.ivr_timeout !== undefined ? acc.ivr_timeout : 10;
            document.getElementById('acc-ivr-default').value = acc.ivr_default || '';
            populateIvrDropdowns(acc.ivr_welcome || "", acc.ivr_menu || {});
        } else {
            ivrFields.style.display = 'none';
            document.getElementById('acc-ivr-default').value = '';
            document.getElementById('acc-ivr-timeout').value = 10;
            populateIvrDropdowns("", {});
        }

        // Load advanced options
        document.getElementById('acc-display-name').value = acc.display_name || '';
        document.getElementById('acc-user-agent').value = acc.user_agent || '';
        document.getElementById('acc-register-expiry').value = acc.register_expiry !== undefined ? acc.register_expiry : 3600;
        document.getElementById('acc-register-retry').value = acc.register_retry_interval !== undefined ? acc.register_retry_interval : 30;
        document.getElementById('acc-proxy').value = acc.proxy || '';
        document.getElementById('acc-early-media').checked = acc.early_media !== undefined ? acc.early_media : true;
        document.getElementById('acc-session-timers').checked = acc.session_timers !== undefined ? acc.session_timers : false;

        // Populate sound cards
        await populateModalAudioDevices(acc.audio_input_device || "", acc.audio_output_device || "");

        document.getElementById('modal-mode-title').innerText = 'Edit SIP Account';
        document.getElementById('account-modal').classList.add('active');
    } catch (e) {
        console.error(e);
    }
}

function closeAccountModal() {
    document.getElementById('account-modal').classList.remove('active');
    const accPwd = document.getElementById('acc-password');
    if (accPwd) accPwd.classList.remove('revealed');
    const pwdToggle = document.getElementById('acc-password-toggle');
    if (pwdToggle) pwdToggle.textContent = '👁️';
}

// Form Submit for Add/Edit
document.getElementById('account-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const originalName = document.getElementById('edit-original-name').value;
    const isEdit = originalName.length > 0;

    const name = document.getElementById('acc-name').value;
    const username = document.getElementById('acc-username').value;
    const password = document.getElementById('acc-password').value;
    const server = document.getElementById('acc-server').value;
    const domain = document.getElementById('acc-domain').value || undefined;
    const sip_port = parseInt(document.getElementById('acc-sip-port').value);
    const codec = document.getElementById('acc-codec').value;
    const transport = document.getElementById('acc-transport').value;
    const auth_method = document.getElementById('acc-auth-method').value;
    const rtp_port_start = parseInt(document.getElementById('acc-rtp-start').value);
    const rtp_port_end = parseInt(document.getElementById('acc-rtp-end').value);
    const auto_answer = document.getElementById('acc-auto-answer').checked;
    
    // Welcome WAV file value from Visual Selector dropdown
    const ivr_welcome = auto_answer ? (document.getElementById('acc-ivr-welcome').value || undefined) : undefined;
    const ivr_timeout = auto_answer ? parseInt(document.getElementById('acc-ivr-timeout').value) : undefined;
    const ivr_default = auto_answer ? (document.getElementById('acc-ivr-default').value || undefined) : undefined;

    // Build the IVR Menu mapping object from our visual rows
    let ivr_menu = undefined;
    if (auto_answer) {
        ivr_menu = {};
        const rows = document.querySelectorAll('#ivr-menu-builder-list .ivr-menu-row');
        rows.forEach(row => {
            const digit = row.getAttribute('data-digit');
            const enabled = row.querySelector('.ivr-row-enable').checked;
            if (enabled) {
                const action = row.querySelector('.ivr-row-action').value;
                if (action === 'hold' || action === 'hangup') {
                    ivr_menu[digit] = action;
                } else if (action === 'playback' || action === 'transfer') {
                    const paramVal = row.querySelector('.ivr-row-param').value;
                    if (paramVal) {
                        ivr_menu[digit] = `${action}:${paramVal}`;
                    }
                } else if (action === 'record') {
                    const file = row.querySelector('.ivr-rec-file').value || 'voicemail.wav';
                    const dur = row.querySelector('.ivr-rec-dur').value || '30';
                    ivr_menu[digit] = `record:${file}:${dur}`;
                }
            }
        });
        if (Object.keys(ivr_menu).length === 0) {
            ivr_menu = undefined; // map can be omitted if empty
        }
    }

    // Advanced & sound card fields
    const audio_input_device = document.getElementById('acc-audio-input').value || undefined;
    const audio_output_device = document.getElementById('acc-audio-output').value || undefined;
    const display_name = document.getElementById('acc-display-name').value || undefined;
    const user_agent = document.getElementById('acc-user-agent').value || undefined;
    const register_expiry = parseInt(document.getElementById('acc-register-expiry').value);
    const register_retry_interval = parseInt(document.getElementById('acc-register-retry').value);
    const proxy = document.getElementById('acc-proxy').value || undefined;
    const early_media = document.getElementById('acc-early-media').checked;
    const session_timers = document.getElementById('acc-session-timers').checked;

    const accountData = {
        name, username, password, server, domain, sip_port, codec,
        transport, auth_method,
        rtp_port_start, rtp_port_end, auto_answer, ivr_welcome,
        ivr_timeout, ivr_menu, ivr_default,
        audio_input_device, audio_output_device,
        display_name, user_agent, 
        register_expiry, register_retry_interval, proxy, early_media, session_timers
    };

    const url = isEdit ? `${API_URL}/api/accounts/${originalName}` : `${API_URL}/api/accounts`;
    const method = isEdit ? 'PUT' : 'POST';

    try {
        const res = await fetch(url, {
            method: method,
            headers: getAuthHeaders(),
            body: JSON.stringify(accountData)
        });

        if (res.ok) {
            showNotification(isEdit ? "Account updated successfully!" : "Account added successfully!", "success");
            closeAccountModal();
            loadAccountsConfig();
            updateDashboard();
        } else {
            showNotification("Failed to save account. Check for duplicate names or values.", "error");
        }
    } catch (err) {
        showNotification("Network error saving configuration.", "error");
    }
});

// Delete account configuration
async function deleteAccount(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}`, {
            method: 'DELETE',
            headers: getAuthHeaders()
        });

        if (res.ok) {
            showNotification("Account deleted successfully!", "success");
            loadAccountsConfig();
            updateDashboard();
        } else {
            showNotification("Failed to delete account.", "error");
        }
    } catch (err) {
        showNotification("Network error deleting account.", "error");
    }
}
