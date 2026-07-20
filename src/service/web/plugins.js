// ====================================================================
// Plugin Manager Web UI Logic (plugins.js)
// ====================================================================

let activePluginScriptFile = "";

async function loadPluginsTab() {
    await fetchPluginsStatus();
}

async function fetchPluginsStatus() {
    try {
        const res = await fetch(`${API_URL}/api/plugins`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        const data = await res.json();

        // Render script files list
        const scriptsList = document.getElementById('plugin-scripts-list');
        if (scriptsList) {
            scriptsList.innerHTML = '';
            if (data.script_files.length === 0) {
                scriptsList.innerHTML = '<div style="font-size:0.85rem; color:var(--text-secondary); padding:0.5rem;">No script files found in plugins/ directory.</div>';
            } else {
                data.script_files.forEach(file => {
                    const isSelected = file === activePluginScriptFile;
                    const item = document.createElement('div');
                    item.className = `script-item ${isSelected ? 'active' : ''}`;
                    item.style.padding = '0.5rem 0.75rem';
                    item.style.borderRadius = '6px';
                    item.style.cursor = 'pointer';
                    item.style.marginBottom = '0.35rem';
                    item.style.background = isSelected ? 'rgba(59, 130, 246, 0.2)' : 'rgba(255,255,255,0.03)';
                    item.style.border = isSelected ? '1px solid var(--accent-primary)' : '1px solid transparent';
                    item.style.display = 'flex';
                    item.style.justifyContent = 'space-between';
                    item.style.alignItems = 'center';

                    const extIcon = file.endsWith('.rhai') ? '🦀 Rhai' : '🌙 Lua';

                    item.innerHTML = `
                        <span style="font-weight: 500; font-size: 0.85rem;">${file}</span>
                        <span style="font-size: 0.75rem; opacity: 0.7; background: rgba(0,0,0,0.3); padding: 0.1rem 0.4rem; border-radius: 4px;">${extIcon}</span>
                    `;

                    item.onclick = () => selectPluginScript(file);
                    scriptsList.appendChild(item);
                });
            }
        }
    } catch (err) {
        console.error("Failed to fetch plugins status:", err);
    }
}

async function selectPluginScript(filename) {
    activePluginScriptFile = filename;
    document.getElementById('plugin-script-title').innerText = filename;
    await fetchPluginsStatus(); // update active selection styling

    try {
        const res = await fetch(`${API_URL}/api/plugins/scripts/${filename}`, { headers: getAuthHeaders() });
        if (res.ok) {
            const data = await res.json();
            document.getElementById('plugin-script-editor').value = data.content || '';
        }
    } catch (err) {
        console.error("Failed to load script content:", err);
    }
}

async function saveCurrentPluginScript() {
    if (!activePluginScriptFile) {
        showNotification("Please select or create a script file first.", "error");
        return;
    }
    const content = document.getElementById('plugin-script-editor').value;

    try {
        const res = await fetch(`${API_URL}/api/plugins/scripts`, {
            method: 'POST',
            headers: getAuthHeaders(),
            body: JSON.stringify({ filename: activePluginScriptFile, content: content })
        });
        const data = await res.json();
        if (data.success) {
            showNotification(`Script '${activePluginScriptFile}' saved successfully!`, "success");
        } else {
            showNotification(`Failed to save script: ${data.msg}`, "error");
        }
    } catch (err) {
        showNotification("Network error saving script.", "error");
    }
}

async function createNewPluginScript() {
    const filename = prompt("Enter script filename (e.g. custom_flow.rhai or custom_flow.lua):");
    if (!filename) return;

    if (!filename.endsWith('.rhai') && !filename.endsWith('.lua')) {
        showNotification("Filename must end with .rhai or .lua", "error");
        return;
    }

    const defaultTemplate = filename.endsWith('.rhai')
        ? `// Rhai script template\nlet caller = context.caller;\nrsip_log("info", "Executing Rhai script for: " + caller);\n#{ action: "none" }\n`
        : `-- Lua script template\nlocal caller = (context and context.caller) or "Unknown"\nrsip.log("info", "Executing Lua script for: " .. caller)\nreturn { action = "none" }\n`;

    try {
        const res = await fetch(`${API_URL}/api/plugins/scripts`, {
            method: 'POST',
            headers: getAuthHeaders(),
            body: JSON.stringify({ filename: filename, content: defaultTemplate })
        });
        const data = await res.json();
        if (data.success) {
            showNotification(`Script '${filename}' created successfully!`, "success");
            activePluginScriptFile = filename;
            await fetchPluginsStatus();
            await selectPluginScript(filename);
        } else {
            showNotification(`Failed to create script: ${data.msg}`, "error");
        }
    } catch (err) {
        showNotification("Error creating script file.", "error");
    }
}
