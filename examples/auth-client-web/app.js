const authState = {
  accessToken: null,
  refreshToken: null,
  user: null,
  sessions: [],
};

let baseUrl = 'http://127.0.0.1:3000';
let clientId = 'peanut-web-dev';

const els = {
  baseUrl: document.querySelector('#baseUrl'),
  clientId: document.querySelector('#clientId'),
  email: document.querySelector('#email'),
  password: document.querySelector('#password'),
  currentPassword: document.querySelector('#currentPassword'),
  newPassword: document.querySelector('#newPassword'),
  resetToken: document.querySelector('#resetToken'),
  output: document.querySelector('#output'),
  stateOutput: document.querySelector('#stateOutput'),
  accessState: document.querySelector('#accessState'),
  refreshState: document.querySelector('#refreshState'),
  clientIdState: document.querySelector('#clientIdState'),
};

function setOutput(value) {
  els.output.textContent = typeof value === 'string' ? value : JSON.stringify(value, null, 2);
}

function renderState() {
  els.accessState.textContent = authState.accessToken ? 'present' : 'empty';
  els.refreshState.textContent = authState.refreshToken ? 'present' : 'empty';
  els.clientIdState.textContent = clientId || 'empty';
  els.stateOutput.textContent = JSON.stringify(
    {
      baseUrl,
      clientId: clientId || null,
      user: authState.user,
      sessions: authState.sessions,
      accessTokenPreview: authState.accessToken ? `${authState.accessToken.slice(0, 24)}...` : null,
      refreshTokenPreview: authState.refreshToken ? `${authState.refreshToken.slice(0, 24)}...` : null,
    },
    null,
    2,
  );
}

function setTokens(payload) {
  authState.accessToken = payload.access_token ?? null;
  authState.refreshToken = payload.refresh_token ?? null;
  authState.user = payload.user ?? authState.user;
  renderState();
}

function clearAuthState() {
  authState.accessToken = null;
  authState.refreshToken = null;
  authState.user = null;
  authState.sessions = [];
  renderState();
}

async function readJson(response) {
  const text = await response.text();
  try {
    return text ? JSON.parse(text) : null;
  } catch {
    return { raw: text };
  }
}

async function rawRequest(path, init = {}) {
  const headers = new Headers(init.headers || {});
  if (!headers.has('Content-Type') && init.body) {
    headers.set('Content-Type', 'application/json');
  }
  if (clientId) {
    headers.set('x-peanut-client-id', clientId);
  }
  if (authState.accessToken) {
    headers.set('Authorization', `Bearer ${authState.accessToken}`);
  }

  const response = await fetch(`${baseUrl}${path}`, {
    ...init,
    headers,
  });

  const data = await readJson(response);
  return { response, data };
}

async function refreshSession() {
  if (!authState.refreshToken) {
    throw new Error('No refresh token available');
  }
  const { response, data } = await rawRequest('/api/auth/refresh', {
    method: 'POST',
    body: JSON.stringify({ refresh_token: authState.refreshToken }),
  });
  if (!response.ok) {
    clearAuthState();
    throw new Error(data?.error || 'refresh failed');
  }
  setTokens(data);
  return data;
}

async function api(path, init = {}, retry = true) {
  const result = await rawRequest(path, init);
  if (result.response.status === 401 && retry && authState.refreshToken) {
    await refreshSession();
    return rawRequest(path, init);
  }
  return result;
}

async function run(label, fn) {
  try {
    const result = await fn();
    setOutput({ label, ok: true, result });
  } catch (error) {
    setOutput({ label, ok: false, error: error.message });
  }
  renderState();
}

async function register() {
  const { response, data } = await rawRequest('/api/register', {
    method: 'POST',
    body: JSON.stringify({
      email: els.email.value,
      password: els.password.value,
    }),
  });
  if (!response.ok) {
    throw new Error(data?.error || 'register failed');
  }
  return data;
}

async function login() {
  const { response, data } = await rawRequest('/api/login', {
    method: 'POST',
    body: JSON.stringify({
      email: els.email.value,
      password: els.password.value,
    }),
  });
  if (!response.ok) {
    throw new Error(data?.error || 'login failed');
  }
  setTokens(data);
  return data;
}

async function loadMe() {
  const { response, data } = await api('/api/me');
  if (!response.ok) {
    throw new Error(data?.error || 'load me failed');
  }
  authState.user = data?.user ?? null;
  return data;
}

async function logout() {
  if (!authState.refreshToken) {
    throw new Error('No refresh token available');
  }
  const { response, data } = await rawRequest('/api/auth/logout', {
    method: 'POST',
    body: JSON.stringify({ refresh_token: authState.refreshToken }),
  });
  clearAuthState();
  if (!response.ok) {
    throw new Error(data?.error || 'logout failed');
  }
  return data;
}

async function listSessions() {
  const { response, data } = await api('/api/auth/sessions');
  if (!response.ok) {
    throw new Error(data?.error || 'list sessions failed');
  }
  authState.sessions = Array.isArray(data?.sessions) ? data.sessions : [];
  return data;
}

async function revokeFirstSession() {
  if (!authState.sessions.length) {
    throw new Error('Load sessions first');
  }
  const first = authState.sessions[0];
  const { response, data } = await api(`/api/auth/sessions/${encodeURIComponent(first.session_id)}`, {
    method: 'DELETE',
  });
  if (!response.ok) {
    throw new Error(data?.error || 'revoke session failed');
  }
  await listSessions();
  return { revoked_session_id: first.session_id, response: data };
}

async function revokeAllSessions() {
  const { response, data } = await api('/api/auth/sessions/revoke-all', {
    method: 'POST',
  });
  if (!response.ok) {
    throw new Error(data?.error || 'revoke all sessions failed');
  }
  authState.sessions = [];
  return data;
}

async function changePassword() {
  const { response, data } = await api('/api/auth/change-password', {
    method: 'POST',
    body: JSON.stringify({
      current_password: els.currentPassword.value,
      new_password: els.newPassword.value,
    }),
  });
  if (!response.ok) {
    throw new Error(data?.error || 'change password failed');
  }
  clearAuthState();
  return {
    ...data,
    note: 'Existing refresh sessions are revoked, so the demo clears in-memory auth state.',
  };
}

async function forgotPassword() {
  const { response, data } = await rawRequest('/api/auth/forgot-password', {
    method: 'POST',
    body: JSON.stringify({ email: els.email.value }),
  });
  if (!response.ok) {
    throw new Error(data?.error || 'forgot password failed');
  }
  if (data?.reset_token) {
    els.resetToken.value = data.reset_token;
  }
  return data;
}

async function resetPassword() {
  const { response, data } = await rawRequest('/api/auth/reset-password', {
    method: 'POST',
    body: JSON.stringify({
      reset_token: els.resetToken.value,
      new_password: els.newPassword.value,
    }),
  });
  clearAuthState();
  if (!response.ok) {
    throw new Error(data?.error || 'reset password failed');
  }
  return data;
}

function saveBaseUrl() {
  baseUrl = (els.baseUrl.value || '').trim().replace(/\/$/, '');
  clientId = (els.clientId.value || '').trim();
  renderState();
  setOutput({ ok: true, baseUrl, clientId: clientId || null });
}

document.querySelector('#saveBaseUrl').addEventListener('click', () => run('saveBaseUrl', saveBaseUrl));
document.querySelector('#register').addEventListener('click', () => run('register', register));
document.querySelector('#login').addEventListener('click', () => run('login', login));
document.querySelector('#loadMe').addEventListener('click', () => run('loadMe', loadMe));
document.querySelector('#refreshSession').addEventListener('click', () => run('refreshSession', refreshSession));
document.querySelector('#logout').addEventListener('click', () => run('logout', logout));
document.querySelector('#listSessions').addEventListener('click', () => run('listSessions', listSessions));
document.querySelector('#revokeFirstSession').addEventListener('click', () => run('revokeFirstSession', revokeFirstSession));
document.querySelector('#revokeAllSessions').addEventListener('click', () => run('revokeAllSessions', revokeAllSessions));
document.querySelector('#changePassword').addEventListener('click', () => run('changePassword', changePassword));
document.querySelector('#forgotPassword').addEventListener('click', () => run('forgotPassword', forgotPassword));
document.querySelector('#resetPassword').addEventListener('click', () => run('resetPassword', resetPassword));

renderState();
setOutput({
  message: 'Ready. Set the Peanut base URL, then register or login.',
  auth_client_policy_hint: 'If AUTH_ALLOWED_CLIENT_IDS is enabled on the server, keep the client id field filled so the example sends x-peanut-client-id.',
  production_note: 'This example stores tokens in memory only. Prefer a BFF or secure cookie strategy for production refresh tokens.',
});
