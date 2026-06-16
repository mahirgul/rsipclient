const API_URL = "";

// Global Toast Notification System
window.showNotification = function(message, type = 'info', duration = 5000) {
    let container = document.getElementById('notification-container');
    if (!container) {
        container = document.createElement('div');
        container.id = 'notification-container';
        document.body.appendChild(container);
    }

    const notification = document.createElement('div');
    notification.className = `notification ${type}`;

    let svgIcon = '';
    if (type === 'success') {
        svgIcon = `<svg class="toast-svg success-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path><polyline points="22 4 12 14.01 9 11.01"></polyline></svg>`;
    } else if (type === 'error') {
        svgIcon = `<svg class="toast-svg error-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="15" y1="9" x2="9" y2="15"></line><line x1="9" y1="9" x2="15" y2="15"></line></svg>`;
    } else if (type === 'warning') {
        svgIcon = `<svg class="toast-svg warning-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>`;
    } else {
        svgIcon = `<svg class="toast-svg info-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="16" x2="12" y2="12"></line><line x1="12" y1="8" x2="12.01" y2="8"></line></svg>`;
    }

    const cleanMsg = (message || '').toString().replace(/[&<>'"]/g, 
        tag => ({
            '&': '&amp;',
            '<': '&lt;',
            '>': '&gt;',
            "'": '&#39;',
            '"': '&quot;'
        }[tag] || tag)
    );

    notification.innerHTML = `
        <div class="notification-icon">${svgIcon}</div>
        <div class="notification-content">${cleanMsg}</div>
        <button class="notification-close">&times;</button>
        <div class="notification-progress" style="animation: shrinkProgress ${duration}ms linear forwards;"></div>
    `;

    const closeBtn = notification.querySelector('.notification-close');
    const removeNotification = () => {
        if (notification.classList.contains('closing')) return;
        notification.classList.add('closing');
        
        // Save current height for transition
        const height = notification.offsetHeight;
        notification.style.height = height + 'px';
        
        // Force reflow
        notification.offsetHeight;
        
        // Transition height and spacing to 0 for a smooth slide-up layout effect
        notification.style.height = '0';
        notification.style.paddingTop = '0';
        notification.style.paddingBottom = '0';
        notification.style.marginTop = '0';
        notification.style.marginBottom = '0';
        notification.style.opacity = '0';
        notification.style.transform = 'translateX(120%)';
        notification.style.borderLeftWidth = '0';
        notification.style.borderRightWidth = '0';
        notification.style.borderTopWidth = '0';
        notification.style.borderBottomWidth = '0';

        setTimeout(() => {
            if (notification.parentNode) {
                notification.remove();
            }
        }, 400);
    };

    closeBtn.addEventListener('click', removeNotification);

    // Pause on Hover Logic
    let closeTimeout = null;
    let startTime = Date.now();
    let remaining = duration;

    const startTimer = (time) => {
        if (time <= 0) return;
        startTime = Date.now();
        closeTimeout = setTimeout(removeNotification, time);
    };

    const stopTimer = () => {
        clearTimeout(closeTimeout);
        remaining -= Date.now() - startTime;
    };

    if (duration > 0) {
        startTimer(duration);

        notification.addEventListener('mouseenter', () => {
            stopTimer();
            const progress = notification.querySelector('.notification-progress');
            if (progress) {
                progress.style.animationPlayState = 'paused';
            }
        });

        notification.addEventListener('mouseleave', () => {
            startTimer(remaining);
            const progress = notification.querySelector('.notification-progress');
            if (progress) {
                progress.style.animationPlayState = 'running';
            }
        });
    }

    container.appendChild(notification);

    // Trigger reflow to start transition
    notification.offsetHeight;
    notification.classList.add('show');
};

function getToken() {
    return localStorage.getItem('sip_session_token');
}

function getAuthHeaders() {
    return {
        'Authorization': `Bearer ${getToken()}`,
        'Content-Type': 'application/json'
    };
}

// Form Submit for login
document.getElementById('login-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const u = document.getElementById('username').value;
    const p = document.getElementById('password').value;
    const errorToast = document.getElementById('login-error');
    errorToast.style.display = 'none';

    try {
        const res = await fetch(`${API_URL}/api/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ username: u, password: p })
        });

        if (res.ok) {
            const data = await res.json();
            localStorage.setItem('sip_session_token', data.token);
            initApp();
        } else {
            errorToast.style.display = 'block';
        }
    } catch (err) {
        console.error(err);
        errorToast.innerText = "Network error. Failed to reach web server.";
        errorToast.style.display = 'block';
    }
});
