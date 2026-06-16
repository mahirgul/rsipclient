// Trigger Auto Answer IVR subfields visibility
document.getElementById('acc-auto-answer').addEventListener('change', (e) => {
    const ivrFields = document.getElementById('ivr-subfields');
    ivrFields.style.display = e.target.checked ? 'block' : 'none';
});

// Load account settings configurations
async function loadAccountsConfig() {
    try {
        const res = await fetch(`${API_URL}/api/accounts`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        const accounts = await res.json();

        const configBody = document.getElementById('accounts-config-body');
        configBody.innerHTML = '';

        if (accounts.length === 0) {
            configBody.innerHTML = `<tr><td colspan="8" style="text-align: center; color: var(--text-secondary);">No accounts found. Create a new one.</td></tr>`;
        } else {
            accounts.forEach(acc => {
                const autoAns = acc.auto_answer ? 'Yes (IVR)' : 'No';
                const tr = document.createElement('tr');
                tr.innerHTML = `
                    <td style="font-weight:600;">${acc.name}</td>
                    <td>${acc.username}</td>
                    <td>${acc.server}</td>
                    <td style="text-transform: uppercase;">${acc.codec || 'pcmu'}</td>
                    <td>${acc.sip_port === 0 ? 'Auto' : acc.sip_port}</td>
                    <td>${acc.rtp_port_start}-${acc.rtp_port_end}</td>
                    <td>${autoAns}</td>
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

// Account addition and modification forms
function openAddAccountModal() {
    document.getElementById('account-form').reset();
    document.getElementById('edit-original-name').value = '';
    document.getElementById('modal-mode-title').innerText = 'Add SIP Account';
    document.getElementById('acc-name').disabled = false;
    document.getElementById('ivr-subfields').style.display = 'none';

    // Reset advanced options to default values
    document.getElementById('acc-ivr-timeout').value = 10;
    document.getElementById('acc-display-name').value = '';
    document.getElementById('acc-user-agent').value = '';
    document.getElementById('acc-register-expiry').value = 3600;
    document.getElementById('acc-register-retry').value = 30;
    document.getElementById('acc-proxy').value = '';
    document.getElementById('acc-early-media').checked = true;
    document.getElementById('acc-session-timers').checked = false;

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
        document.getElementById('acc-server').value = acc.server;
        document.getElementById('acc-domain').value = acc.domain || '';
        document.getElementById('acc-sip-port').value = acc.sip_port;
        document.getElementById('acc-codec').value = acc.codec || 'pcmu';
        document.getElementById('acc-rtp-start').value = acc.rtp_port_start;
        document.getElementById('acc-rtp-end').value = acc.rtp_port_end;
        document.getElementById('acc-transport').value = acc.transport || 'udp';
        document.getElementById('acc-auth-method').value = acc.auth_method || 'md5';
        document.getElementById('acc-auto-answer').checked = acc.auto_answer || false;

        const ivrFields = document.getElementById('ivr-subfields');
        if (acc.auto_answer) {
            ivrFields.style.display = 'block';
            document.getElementById('acc-ivr-welcome').value = acc.ivr_welcome || '';
            document.getElementById('acc-ivr-timeout').value = acc.ivr_timeout !== undefined ? acc.ivr_timeout : 10;
        } else {
            ivrFields.style.display = 'none';
            document.getElementById('acc-ivr-welcome').value = '';
            document.getElementById('acc-ivr-timeout').value = 10;
        }

        // Load advanced options
        document.getElementById('acc-display-name').value = acc.display_name || '';
        document.getElementById('acc-user-agent').value = acc.user_agent || '';
        document.getElementById('acc-register-expiry').value = acc.register_expiry !== undefined ? acc.register_expiry : 3600;
        document.getElementById('acc-register-retry').value = acc.register_retry_interval !== undefined ? acc.register_retry_interval : 30;
        document.getElementById('acc-proxy').value = acc.proxy || '';
        document.getElementById('acc-early-media').checked = acc.early_media !== undefined ? acc.early_media : true;
        document.getElementById('acc-session-timers').checked = acc.session_timers !== undefined ? acc.session_timers : false;

        document.getElementById('modal-mode-title').innerText = 'Edit SIP Account';
        document.getElementById('account-modal').classList.add('active');
    } catch (e) {
        console.error(e);
    }
}

function closeAccountModal() {
    document.getElementById('account-modal').classList.remove('active');
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
    const ivr_welcome = auto_answer ? (document.getElementById('acc-ivr-welcome').value || undefined) : undefined;

    // Advanced & IVR fields
    const ivr_timeout = auto_answer ? parseInt(document.getElementById('acc-ivr-timeout').value) : undefined;
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
        ivr_timeout, display_name, user_agent, register_expiry,
        register_retry_interval, proxy, early_media, session_timers
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
