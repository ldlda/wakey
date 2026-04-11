const $ = (id) => document.getElementById(id);

function setStatus(kind, text) {
  const pill = $("status-pill");
  pill.className = `pill ${kind}`;
  pill.textContent = text;
}

async function api(path, init) {
  const res = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    let body = "";
    try { body = JSON.stringify(await res.json(), null, 2); } catch (_) {}
    throw new Error(`${res.status} ${res.statusText}\n${body}`);
  }
  return res.json();
}

function renderAgents(agents) {
  const root = $("agents");
  root.innerHTML = "";
  if (!agents.length) {
    root.innerHTML = '<div class="item">No agents enrolled yet</div>';
    return;
  }
  for (const a of agents) {
    const row = document.createElement("div");
    row.className = "item";
    row.innerHTML = `<span>${a.agent_id}</span><span>${a.connected ? "connected" : "offline"}</span>`;
    row.onclick = () => { $("agent-id").value = a.agent_id; };
    root.appendChild(row);
  }
}

function renderAlerts(alerts) {
  const root = $("alerts");
  root.innerHTML = "";
  if (!alerts.length) {
    root.innerHTML = '<div class="item">No active alerts</div>';
    return;
  }
  for (const a of alerts) {
    const row = document.createElement("div");
    row.className = "item";
    row.innerHTML = `<span>${a.kind}${a.agent_id ? ` (${a.agent_id})` : ""}</span><span>${a.severity}</span>`;
    root.appendChild(row);
  }
}

async function loadAgents() {
  const agents = await api("/api/v1/control/agents");
  renderAgents(agents);
  return agents;
}

async function loadAlerts() {
  const alerts = await api("/api/v1/control/alerts");
  renderAlerts(alerts);
  return alerts;
}

async function loadAudit() {
  const events = await api("/api/v1/control/audit/events?limit=25");
  $("audit").textContent = JSON.stringify(events, null, 2);
}

function buildCommandPayload(kind, query) {
  if (kind === "devs") {
    return { command: { kind: "devs", dev: null, up_only: false } };
  }
  if (kind === "status") {
    return { command: { kind: "status", query: query || null, name: null, ips: [], devs: [], nuds: [], macs: [] } };
  }
  if (kind === "leases") {
    return { command: { kind: "leases", include_state: true } };
  }
  if (kind === "inventory") {
    return { command: { kind: "inventory", query: query || null, name: null, ips: [], devs: [], nuds: [], macs: [] } };
  }
  return { command: { kind: "wake", query: query || null, mac: null, ip: null } };
}

async function runCommand(evt) {
  evt.preventDefault();
  const agentId = $("agent-id").value.trim();
  const kind = $("command-kind").value;
  const query = $("command-query").value.trim();
  if (!agentId) return;

  const payload = buildCommandPayload(kind, query);
  const out = $("command-result");
  out.textContent = "Running...";
  try {
    const result = await api(`/api/v1/control/agents/${encodeURIComponent(agentId)}/command`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
    out.textContent = JSON.stringify(result, null, 2);
    setStatus("ok", "Command OK");
    await loadAudit();
  } catch (err) {
    out.textContent = String(err);
    setStatus("bad", "Command Failed");
  }
}

async function bootstrap() {
  try {
    setStatus("warn", "Loading");
    await Promise.all([loadAgents(), loadAlerts(), loadAudit()]);
    setStatus("ok", "Ready");
  } catch (err) {
    setStatus("bad", "API Error");
    $("audit").textContent = String(err);
  }
}

$("refresh-agents").onclick = () => loadAgents().catch((e) => { setStatus("bad", "Agent Error"); console.error(e); });
$("refresh-alerts").onclick = () => loadAlerts().catch((e) => { setStatus("bad", "Alerts Error"); console.error(e); });
$("refresh-audit").onclick = () => loadAudit().catch((e) => { setStatus("bad", "Audit Error"); console.error(e); });
$("command-form").onsubmit = runCommand;

bootstrap();
