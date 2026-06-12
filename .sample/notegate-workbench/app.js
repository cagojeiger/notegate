const PAGE_SIZE = 8;

const sample = {
  spaces: [
    { id: "personal", name: "Personal", short: "P" },
    { id: "lab", name: "Research Lab", short: "R" },
    { id: "agent", name: "Agent Vault", short: "A" },
    { id: "archive", name: "Archive", short: "Z" },
    { id: "drafts", name: "Drafts", short: "D" },
    { id: "work", name: "Work", short: "W" },
    { id: "books", name: "Books", short: "B" },
    { id: "ideas", name: "Ideas", short: "I" },
    { id: "ops", name: "Ops", short: "O" },
    { id: "tmp", name: "Tmp", short: "T" },
    { id: "long", name: "Long-term", short: "L" },
  ],
  nodes: {
    personal: [
      { id: "root", parent: null, kind: "folder", name: "/", path: "/", updated: "2026-06-12", children: true },
      { id: "daily", parent: "root", kind: "folder", name: "daily", path: "/daily", updated: "2026-06-12", children: true },
      { id: "projects", parent: "root", kind: "folder", name: "projects", path: "/projects", updated: "2026-06-11", children: true },
      { id: "assets", parent: "root", kind: "folder", name: "assets", path: "/assets", updated: "2026-06-09", children: true },
      { id: "inbox", parent: "root", kind: "text", name: "inbox.md", path: "/inbox.md", updated: "2026-06-12", bytes: 924, lines: 32, content: "# Inbox\n\n- TODO: review Notegate UI sample\n- Capture ideas from mobile layout\n- Ask agent to summarize notes\n\nThis document represents a plain Text node.\n" },
      { id: "daily-1", parent: "daily", kind: "text", name: "2026-06-12.md", path: "/daily/2026-06-12.md", updated: "2026-06-12", bytes: 1440, lines: 58, content: "# 2026-06-12\n\n## Notes\n\nNotegate UI should feel like a focused personal workspace.\n\n## Decisions\n\n- ActivityRail is a scrollable Space rail.\n- PrimarySidebar owns Tree and Recent.\n- AuxiliarySidebar starts as Inspector and Agent.\n" },
      { id: "daily-2", parent: "daily", kind: "text", name: "2026-06-11.md", path: "/daily/2026-06-11.md", updated: "2026-06-11", bytes: 820, lines: 27, content: "# 2026-06-11\n\nWorked on API key policy and UI terminology.\n" },
      { id: "project-alpha", parent: "projects", kind: "folder", name: "alpha", path: "/projects/alpha", updated: "2026-06-10", children: true },
      { id: "project-beta", parent: "projects", kind: "folder", name: "beta", path: "/projects/beta", updated: "2026-06-08", children: true },
      { id: "alpha-readme", parent: "project-alpha", kind: "text", name: "README.md", path: "/projects/alpha/README.md", updated: "2026-06-10", bytes: 1240, lines: 42, content: "# Project Alpha\n\nA prototype for AI-native personal storage.\n\n## Tasks\n\n- Validate UI information architecture\n- Keep the sample interactive\n" },
      { id: "alpha-spec", parent: "project-alpha", kind: "text", name: "spec.json", path: "/projects/alpha/spec.json", updated: "2026-06-09", bytes: 360, lines: 12, content: "{\n  \"name\": \"alpha\",\n  \"status\": \"draft\",\n  \"owner\": \"user\"\n}\n" },
      { id: "logo", parent: "assets", kind: "file", name: "logo.png", path: "/assets/logo.png", updated: "2026-06-06", bytes: 38220, media: "image/png" },
      { id: "report", parent: "assets", kind: "file", name: "report.pdf", path: "/assets/report.pdf", updated: "2026-06-04", bytes: 182044, media: "application/pdf" },
      ...Array.from({ length: 18 }, (_, i) => ({
        id: `ref-${i + 1}`,
        parent: "projects",
        kind: "text",
        name: `reference-${String(i + 1).padStart(2, "0")}.md`,
        path: `/projects/reference-${String(i + 1).padStart(2, "0")}.md`,
        updated: `2026-05-${String(28 - (i % 20)).padStart(2, "0")}`,
        bytes: 520 + i * 13,
        lines: 18 + i,
        content: `# Reference ${i + 1}\n\nThis item exists to demonstrate folder pagination.\n\nPage size in the sample is ${PAGE_SIZE}.\n`,
      })),
    ],
  },
};

for (const space of sample.spaces) {
  if (!sample.nodes[space.id]) {
    sample.nodes[space.id] = [
      { id: `${space.id}-root`, parent: null, kind: "folder", name: "/", path: "/", updated: "2026-06-10", children: true },
      { id: `${space.id}-note`, parent: `${space.id}-root`, kind: "text", name: "welcome.md", path: "/welcome.md", updated: "2026-06-10", bytes: 420, lines: 16, content: `# ${space.name}\n\nThis is a sample space.\n` },
    ];
  }
}

const state = {
  activeSpace: "personal",
  activeGroup: 0,
  editorGroups: [
    { nodeId: "daily-1", mode: "preview" },
  ],
  recentOpen: true,
  recentView: "list",
  sidebarSplit: 68,
  focusList: [],
  auxTab: "inspector",
  primaryVisible: true,
  primaryWidth: 288,
  auxVisible: true,
  mobileTreeOpen: false,
  mobileAuxOpen: false,
  expanded: new Set(["root", "daily", "projects", "project-alpha"]),
  pages: { root: 1, daily: 1, projects: 1, assets: 1, "project-alpha": 1 },
  contextMenu: null,
};

const app = document.getElementById("app");

function nodes() { return sample.nodes[state.activeSpace] || []; }
function byId(id) { return nodes().find((node) => node.id === id); }
function activeEditorGroup() { return state.editorGroups[state.activeGroup] || state.editorGroups[0]; }
function activeNode() { return byId(activeEditorGroup()?.nodeId) || rootNode(); }
function splitIcon() { return ["▯", "▥", "▦"][Math.min(state.editorGroups.length, 3) - 1]; }
function nextNodeForNewGroup() {
  const open = new Set(state.editorGroups.map((group) => group.nodeId));
  return nodes().find((node) => node.kind === "text" && !open.has(node.id)) || activeNode() || rootNode();
}
function addEditorGroup() {
  if (state.editorGroups.length >= 3) return;
  const node = nextNodeForNewGroup();
  state.editorGroups.push({ nodeId: node?.id, mode: "preview" });
  state.activeGroup = state.editorGroups.length - 1;
  render();
}
function closeEditorGroup(index) {
  if (state.editorGroups.length <= 1) return;
  state.editorGroups.splice(index, 1);
  state.activeGroup = Math.max(0, Math.min(state.activeGroup, state.editorGroups.length - 1));
  render();
}
function rootNode() { return nodes().find((node) => node.parent === null); }
function childrenOf(id) { return nodes().filter((node) => node.parent === id).sort((a, b) => a.name.localeCompare(b.name)); }
function icon(kind) { return kind === "folder" ? "📁" : kind === "file" ? "📦" : "📄"; }
function fmtBytes(bytes = 0) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}
function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[ch]));
}
function markdownPreview(text = "") {
  return text.split("\n").map((line) => {
    if (line.startsWith("# ")) return `<h1>${escapeHtml(line.slice(2))}</h1>`;
    if (line.startsWith("## ")) return `<h2>${escapeHtml(line.slice(3))}</h2>`;
    if (line.startsWith("- ")) return `<p>• ${escapeHtml(line.slice(2))}</p>`;
    if (!line.trim()) return "";
    return `<p>${escapeHtml(line)}</p>`;
  }).join("");
}

function selectSpace(id) {
  state.activeSpace = id;
  const root = rootNode();
  const firstText = nodes().find((node) => node.kind === "text") || root;
  state.editorGroups = [
    { nodeId: firstText?.id || root?.id, mode: "preview" },
  ];
  state.activeGroup = 0;
  state.expanded = new Set(root ? [root.id] : []);
  state.pages = root ? { [root.id]: 1 } : {};
  state.mobileTreeOpen = false;
  render();
}
function selectNode(id) {
  const node = byId(id);
  if (node?.kind === "folder") {
    toggleExpand(id);
    requestAnimationFrame(() => document.querySelector(".primary-sidebar")?.focus());
    return;
  }
  activeEditorGroup().nodeId = id;
  state.mobileTreeOpen = false;
  render();
  requestAnimationFrame(() => document.querySelector(".primary-sidebar")?.focus());
}
function toggleExpand(id) {
  if (state.expanded.has(id)) state.expanded.delete(id);
  else state.expanded.add(id);
  render();
}
function loadMore(id) {
  state.pages[id] = (state.pages[id] || 1) + 1;
  render();
}

function today() { return "2026-06-12"; }
function uniqueChildName(parentId, desired) {
  const siblings = new Set(childrenOf(parentId).map((node) => node.name));
  if (!siblings.has(desired)) return desired;
  const dot = desired.lastIndexOf(".");
  const stem = dot > 0 ? desired.slice(0, dot) : desired;
  const ext = dot > 0 ? desired.slice(dot) : "";
  let index = 2;
  while (siblings.has(`${stem}-${index}${ext}`)) index += 1;
  return `${stem}-${index}${ext}`;
}
function childPath(parent, name) {
  if (!parent || parent.path === "/") return `/${name}`;
  return `${parent.path}/${name}`;
}
function createNode(kind, parentId) {
  const parent = byId(parentId) || rootNode();
  if (!parent || parent.kind !== "folder") return;
  const base = kind === "folder" ? "new-folder" : kind === "file" ? "upload.bin" : "untitled.md";
  const name = uniqueChildName(parent.id, base);
  const node = {
    id: `sample-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    parent: parent.id,
    kind,
    name,
    path: childPath(parent, name),
    updated: today(),
  };
  if (kind === "folder") node.children = true;
  if (kind === "text") Object.assign(node, { bytes: 38, lines: 3, content: `# ${name}\n\nNew sample text node.\n` });
  if (kind === "file") Object.assign(node, { bytes: 24576, media: "application/octet-stream" });
  sample.nodes[state.activeSpace].push(node);
  parent.children = true;
  state.expanded.add(parent.id);
  state.pages[parent.id] = Math.max(state.pages[parent.id] || 1, 1);
  if (kind !== "folder") activeEditorGroup().nodeId = node.id;
  else activeEditorGroup().nodeId = node.id;
  state.contextMenu = null;
  render();
}
function contextTargetFromNode(node) {
  if (!node) return rootNode();
  if (node.kind === "folder") return node;
  return byId(node.parent) || rootNode();
}

function selectedCreateParent() {
  return contextTargetFromNode(activeNode());
}
function openCreateMenuAt(anchor) {
  const parent = selectedCreateParent();
  const rect = anchor.getBoundingClientRect();
  state.contextMenu = {
    x: rect.right - 190,
    y: rect.bottom + 6,
    nodeId: parent?.id,
    parentId: parent?.id,
    mode: "create",
  };
  render();
}

function nextPagedFolder(id = rootNode()?.id) {
  if (!id) return null;
  const node = byId(id);
  if (!node || node.kind !== "folder" || !state.expanded.has(id)) return null;
  const kids = childrenOf(id);
  const page = state.pages[id] || 1;
  if (kids.length > page * PAGE_SIZE) return id;
  for (const child of kids) {
    const found = nextPagedFolder(child.id);
    if (found) return found;
  }
  return null;
}

function maybeAutoPageTree(scroller) {
  if (!scroller.classList.contains("tree-scroll")) return;
  const remaining = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
  if (remaining > 80) return;
  const next = nextPagedFolder();
  if (next) loadMore(next);
}

function renderSpaces() {
  return sample.spaces.map((space) => `
    <button class="space-dot ${space.id === state.activeSpace ? "active" : ""}" title="${escapeHtml(space.name)}" data-action="space" data-id="${space.id}">
      ${escapeHtml(space.short)}
    </button>
  `).join("");
}

function renderTreeNode(node, level = 0) {
  const kids = childrenOf(node.id);
  const isFolder = node.kind === "folder";
  const expanded = state.expanded.has(node.id);
  const selected = activeEditorGroup()?.nodeId === node.id;
  let html = `
    <button class="node-row tree-level ${selected ? "selected" : ""}" style="--level:${level}" data-action="select" data-id="${node.id}" data-node-id="${node.id}">
      <span class="twisty" data-action="toggle" data-id="${node.id}">${isFolder ? (expanded ? "▾" : "▸") : ""}</span>
      <span>${icon(node.kind)}</span>
      <span class="node-name">${escapeHtml(node.name)}</span>
      <span class="node-meta">${node.kind === "text" ? `${node.lines}l` : node.kind === "file" ? fmtBytes(node.bytes) : ""}</span>
    </button>`;
  if (isFolder && expanded) {
    const page = state.pages[node.id] || 1;
    const visible = kids.slice(0, page * PAGE_SIZE);
    html += visible.map((child) => renderTreeNode(child, level + 1)).join("");
    if (visible.length < kids.length) {
      html += `<div class="pagination-hint tree-level" style="--level:${level + 1}">Scroll to load ${visible.length}/${kids.length}</div>`;
    }
  }
  return html;
}

function renderTree() {
  const root = rootNode();
  if (!root) return `<div class="empty">No nodes</div>`;
  return renderTreeNode(root, 0);
}

function renderRecent() {
  const recent = nodes().filter((node) => node.parent !== null).sort((a, b) => b.updated.localeCompare(a.updated)).slice(0, 18);
  return recent.map((node) => `
    <button class="recent-row ${state.recentView === "compact" ? "compact" : ""} ${activeEditorGroup()?.nodeId === node.id ? "selected" : ""}" data-action="select" data-id="${node.id}" data-node-id="${node.id}">
      <span>${icon(node.kind)}</span>
      <span>
        <span class="recent-title">${escapeHtml(node.name)}</span>${state.recentView === "compact" ? "" : `<br /><span class="recent-path">${escapeHtml(node.path)} · ${node.updated}</span>`}
      </span>
    </button>
  `).join("");
}

function renderSidebar() {
  const space = sample.spaces.find((s) => s.id === state.activeSpace);
  return `
    <aside class="primary-sidebar ${state.primaryVisible ? "" : "hidden"} ${state.mobileTreeOpen ? "open" : ""}" tabindex="0" data-action="focus-sidebar">
      <div class="sidebar-header">
        <div class="sidebar-title">
          <div>
            <h2>${escapeHtml(space?.name)}</h2>
            <span>active space</span>
          </div>
          <button class="icon-button compact" title="Create in selected folder" data-action="open-create-menu">＋</button>
        </div>
      </div>
      <div class="sidebar-content" style="--tree-ratio:${state.sidebarSplit}">
        <section class="sidebar-section tree-section">
          <div class="section-header">
            <button class="section-title" data-action="toggle-section" data-section="tree"><span>▾</span><strong>Tree</strong></button>
            <button class="section-action" title="Collapse all folders" data-action="collapse-tree">⇤</button>
          </div>
          <div class="section-body section-scroll tree-scroll">${renderTree()}</div>
        </section>
        <div class="sidebar-resizer" data-action="resize-sidebar" title="Drag to resize Tree/Recent"></div>
        <section class="sidebar-section recent-section">
          <div class="section-header">
            <button class="section-title" data-action="toggle-section" data-section="recent"><span>${state.recentOpen ? "▾" : "▸"}</span><strong>Recent</strong></button>
            <button class="section-action" title="Toggle recent density" data-action="toggle-recent-view">${state.recentView === "compact" ? "☰" : "≡"}</button>
          </div>
          ${state.recentOpen ? `<div class="section-body section-scroll recent-scroll">${renderRecent()}</div>` : ""}
        </section>
      </div>
      <div class="primary-resizer" data-action="resize-primary" title="Drag to resize sidebar"></div>
    </aside>
  `;
}


function renderTextSurface(node, mode) {
  if (!node) return `<section class="preview-pane"><div class="empty">No document</div></section>`;
  if (mode === "edit" && node.kind === "text") {
    return `<section class="editor-pane"><textarea class="editor-textarea" spellcheck="false">${escapeHtml(node.content || "")}</textarea></section>`;
  }
  return `<section class="preview-pane"><article class="preview-card">${markdownPreview(node.content || "")}</article></section>`;
}

function renderNodeSurface(node, groupIndex) {
  if (!node) return `<section class="preview-pane"><div class="empty">No document</div></section>`;
  const group = state.editorGroups[groupIndex];
  if (node.kind === "text") return renderTextSurface(node, group.mode);
  if (node.kind === "file") {
    return `
      <section class="file-pane">
        <div class="info-card"><h3>${icon(node.kind)} ${escapeHtml(node.name)}</h3><div class="key-value"><strong>Path</strong><span>${escapeHtml(node.path)}</span><strong>Media</strong><span>${escapeHtml(node.media)}</span><strong>Size</strong><span>${fmtBytes(node.bytes)}</span></div></div>
        <button class="pill-button">Download</button>
      </section>`;
  }
  const kids = childrenOf(node.id);
  return `
    <section class="folder-pane">
      <div class="info-card"><h3>📁 ${escapeHtml(node.path)}</h3><p class="agent-message">Folder selected. Use the sidebar tree to browse children or create a new Text/File.</p><div class="key-value"><strong>Children</strong><span>${kids.length}</span><strong>Updated</strong><span>${node.updated}</span></div></div>
    </section>`;
}


function renderEditorInfoBar(node) {
  const parts = [
    `<span>${escapeHtml(node.path)}</span>`,
    `<span>${escapeHtml(node.kind)}</span>`,
    node.bytes ? `<span>${fmtBytes(node.bytes)}</span>` : "",
    node.lines ? `<span>${node.lines} lines</span>` : "",
    node.updated ? `<span>updated ${node.updated}</span>` : "",
  ].filter(Boolean).join("");
  return `<div class="editor-info-bar">${parts}</div>`;
}

function renderEditorGroup(groupIndex) {
  const group = state.editorGroups[groupIndex];
  const node = byId(group?.nodeId) || rootNode();
  const active = state.activeGroup === groupIndex;
  if (!node) return `<section class="editor-group ${active ? "active" : ""}" data-action="focus-group" data-group="${groupIndex}"><div class="empty">No space selected</div></section>`;
  return `
    <section class="editor-group ${active ? "active" : ""}" data-action="focus-group" data-group="${groupIndex}">
      <div class="editor-group-header">
        <div class="node-identity">
          <span class="node-icon">${icon(node.kind)}</span>
          <span class="node-title">${escapeHtml(node.name === "/" ? "Space root" : node.name)}</span>
        </div>
        <div class="group-actions">
          ${node.kind === "text" ? `<button class="pill-button compact" data-action="mode" data-group="${groupIndex}">${group.mode === "preview" ? "Edit" : "Preview"}</button>` : ""}
          ${state.editorGroups.length > 1 ? `<button class="icon-button compact" title="Close editor group" data-action="close-group" data-group="${groupIndex}">×</button>` : ""}
        </div>
      </div>
      <div class="editor-viewport">${renderNodeSurface(node, groupIndex)}</div>
      ${renderEditorInfoBar(node)}
    </section>`;
}

function renderEditor() {
  return `
    <main class="editor-area" style="--editor-groups:${state.editorGroups.length}">
      ${state.editorGroups.map((_, index) => renderEditorGroup(index)).join("")}
    </main>`;
}

function renderAux() {
  const node = activeNode();
  if (!node) return "";
  return `
    <aside class="auxiliary-sidebar ${state.auxVisible ? "" : "hidden"} ${state.mobileAuxOpen ? "open" : ""}">
      <div class="aux-tabs">
        <button class="tab-button ${state.auxTab === "inspector" ? "active" : ""}" data-action="aux-tab" data-tab="inspector">Inspector</button>
        <button class="tab-button ${state.auxTab === "agent" ? "active" : ""}" data-action="aux-tab" data-tab="agent">Agent</button>
      </div>
      <div class="aux-content">
        ${state.auxTab === "inspector" ? renderInspector(node) : renderAgent(node)}
      </div>
    </aside>`;
}

function renderInspector(node) {
  return `
    <section class="panel-section"><h3>Node</h3><div class="key-value"><strong>Kind</strong><span>${node.kind}</span><strong>Path</strong><span>${escapeHtml(node.path)}</span><strong>Updated</strong><span>${node.updated}</span>${node.bytes ? `<strong>Bytes</strong><span>${fmtBytes(node.bytes)}</span>` : ""}${node.lines ? `<strong>Lines</strong><span>${node.lines}</span>` : ""}</div></section>
    <section class="panel-section"><h3>Metadata</h3><div class="metadata-box">${escapeHtml(JSON.stringify({ title: node.name, status: node.kind === "folder" ? "container" : "draft", tags: node.kind === "text" ? ["note", "sample"] : [node.kind] }, null, 2))}</div></section>
    <section class="panel-section"><h3>Policy</h3><p class="agent-message">Metadata is not encrypted content. Sensitive values should stay inside encrypted Text or local client state.</p></section>
  `;
}
function renderAgent(node) {
  return `
    <section class="panel-section"><h3>Agent context</h3><p class="agent-message">Current target: <strong>${escapeHtml(node.path)}</strong></p></section>
    <section class="panel-section"><h3>Draft prompt</h3><textarea class="editor-textarea" style="min-height:110px;background:var(--surface);border:1px solid var(--border);border-radius:10px;padding:10px;">Summarize this node and suggest next actions.</textarea></section>
    <button class="pill-button">Run agent</button>
  `;
}


function renderContextMenu() {
  const menu = state.contextMenu;
  if (!menu) return "";
  const node = byId(menu.nodeId);
  const parent = byId(menu.parentId) || rootNode();
  const left = Math.max(8, Math.min(menu.x, window.innerWidth - 210));
  const top = Math.max(8, Math.min(menu.y, window.innerHeight - 170));
  if (menu.mode === "create") {
    return `
      <div class="context-menu" style="left:${left}px;top:${top}px" role="menu">
        <div class="context-menu-title">Create in ${escapeHtml(parent?.path || "/")}</div>
        <button data-action="create-node" data-kind="folder" data-parent="${escapeHtml(parent?.id)}">📁 New Folder</button>
        <button data-action="create-node" data-kind="text" data-parent="${escapeHtml(parent?.id)}">📄 New Text</button>
        <button data-action="create-node" data-kind="file" data-parent="${escapeHtml(parent?.id)}">📦 Upload File</button>
      </div>`;
  }
  return `
    <div class="context-menu" style="left:${left}px;top:${top}px" role="menu">
      <div class="context-menu-title">${escapeHtml(node?.path || "/")}</div>
      <button data-action="select" data-id="${escapeHtml(node?.id)}">Open</button>
      <button data-action="copy-path" data-path="${escapeHtml(node?.path || "/")}">Copy Path</button>
    </div>`;
}

function render() {
  const node = activeNode();
  app.innerHTML = `
    <div class="app-shell">
      <header class="title-bar">
        <div class="brand">
          <button class="icon-button mobile-only" data-action="mobile-tree">☰</button>
          <div class="logo">N</div>
          <div class="brand-text"><span class="brand-title">Notegate</span></div>
        </div>
        <div class="title-center" aria-hidden="true"></div>
        <div class="title-actions">
          <div class="layout-controls" aria-label="Layout controls">
            <button class="mini-layout-button ${state.primaryVisible ? "active" : ""}" title="Toggle primary sidebar" data-action="toggle-primary">◧</button>
            <button class="mini-layout-button ${state.editorGroups.length > 1 ? "active" : ""}" title="${state.editorGroups.length >= 3 ? "Maximum 3 editor groups" : "Add editor group to the right (max 3)"}" data-action="add-group" ${state.editorGroups.length >= 3 ? "disabled" : ""}>${splitIcon()}</button>
            <button class="mini-layout-button ${state.auxVisible ? "active" : ""}" title="Toggle auxiliary sidebar" data-action="toggle-aux">◨</button>
          </div>
          <button class="icon-button mobile-only" data-action="mobile-aux">ⓘ</button>
        </div>
      </header>
      <div class="workbench ${state.primaryVisible ? "" : "primary-hidden"} ${state.auxVisible ? "" : "aux-hidden"}" style="--primary:${state.primaryWidth}px">
        <nav class="activity-rail"><div class="space-rail-list">${renderSpaces()}</div><div class="space-add"><button class="space-dot add-space" title="Create space" data-action="new-space">＋</button></div><div class="rail-footer"><button class="space-dot" title="Settings">⚙</button></div></nav>
        ${renderSidebar()}
        ${renderEditor()}
        ${renderAux()}
      </div>
      <footer class="status-bar"><div class="status-group"><span class="status-item"><span class="status-dot"></span>saved</span><span class="status-item">${escapeHtml(sample.spaces.find((s) => s.id === state.activeSpace)?.name || "")}</span></div></footer>
      ${renderContextMenu()}
      <div class="overlay-backdrop ${state.mobileTreeOpen || state.mobileAuxOpen ? "visible" : ""}" data-action="close-mobile"></div>
    </div>`;
}

app.addEventListener("scroll", (event) => {
  const scroller = event.target.closest?.(".section-scroll");
  if (scroller) maybeAutoPageTree(scroller);
}, true);

app.addEventListener("pointerdown", (event) => {
  const primary = event.target.closest("[data-action='resize-primary']");
  if (primary) return startPrimaryResize(event);
  const sidebar = event.target.closest("[data-action='resize-sidebar']");
  if (sidebar) return startSidebarResize(event);
});


app.addEventListener("contextmenu", (event) => {
  const tree = event.target.closest?.(".tree-scroll");
  if (!tree) return;
  event.preventDefault();
  const row = event.target.closest(".node-row");
  const node = row ? byId(row.dataset.nodeId) : rootNode();
  const parent = contextTargetFromNode(node);
  state.contextMenu = {
    x: event.clientX,
    y: event.clientY,
    nodeId: node?.id,
    parentId: parent?.id,
    mode: !row || node?.kind === "folder" ? "create" : "node",
  };
  render();
});

app.addEventListener("click", (event) => {
  const el = event.target.closest("[data-action]");
  if (!el) { if (state.contextMenu) { state.contextMenu = null; render(); } return; }
  const action = el.dataset.action;
  event.preventDefault();
  event.stopPropagation();
  if (action !== "resize-sidebar" && action !== "resize-primary") state.contextMenu = null;
  if (action === "resize-sidebar" || action === "resize-primary") return;
  if (action === "open-create-menu") return openCreateMenuAt(el);
  if (action === "space") return selectSpace(el.dataset.id);
  if (action === "select") return selectNode(el.dataset.id);
  if (action === "toggle") return toggleExpand(el.dataset.id);
  if (action === "toggle-section") { if (el.dataset.section === "recent") state.recentOpen = !state.recentOpen; return render(); }
  if (action === "collapse-tree") { const root = rootNode(); state.expanded = new Set(root ? [root.id] : []); return render(); }
  if (action === "toggle-recent-view") { state.recentView = state.recentView === "list" ? "compact" : "list"; return render(); }
  if (action === "aux-tab") { state.auxTab = el.dataset.tab; return render(); }
  if (action === "mode") { const group = state.editorGroups[Number(el.dataset.group ?? state.activeGroup)]; group.mode = group.mode === "preview" ? "edit" : "preview"; return render(); }
  if (action === "focus-group") { state.activeGroup = Number(el.dataset.group || 0); return render(); }
  if (action === "add-group") return addEditorGroup();
  if (action === "close-group") return closeEditorGroup(Number(el.dataset.group || 0));
  if (action === "toggle-primary") { state.primaryVisible = !state.primaryVisible; return render(); }
  if (action === "toggle-aux") { state.auxVisible = !state.auxVisible; state.mobileAuxOpen = false; return render(); }
  if (action === "aux") { state.activeGroup = Number(el.dataset.group ?? state.activeGroup); state.auxVisible = !state.auxVisible; state.mobileAuxOpen = state.auxVisible; return render(); }
  if (action === "mobile-tree") { state.mobileTreeOpen = true; return render(); }
  if (action === "mobile-aux") { state.auxVisible = true; state.mobileAuxOpen = true; return render(); }
  if (action === "close-mobile") { state.mobileTreeOpen = false; state.mobileAuxOpen = false; return render(); }
  if (action === "create-node") return createNode(el.dataset.kind, el.dataset.parent);
  if (action === "copy-path") { navigator.clipboard?.writeText(el.dataset.path || ""); return render(); }
  if (action === "new-space") { return render(); }
});



let sidebarResize = null;
let primaryResize = null;


function startPrimaryResize(event) {
  primaryResize = { startX: event.clientX, startWidth: state.primaryWidth };
  document.body.classList.add('is-resizing-primary');
  event.preventDefault();
}

function updatePrimaryResize(event) {
  if (!primaryResize) return;
  const next = primaryResize.startWidth + (event.clientX - primaryResize.startX);
  state.primaryWidth = Math.max(220, Math.min(520, Math.round(next)));
  const workbench = document.querySelector('.workbench');
  if (workbench) workbench.style.setProperty('--primary', `${state.primaryWidth}px`);
}

function stopPrimaryResize() {
  if (!primaryResize) return;
  primaryResize = null;
  document.body.classList.remove('is-resizing-primary');
}

document.addEventListener('pointermove', updatePrimaryResize);
document.addEventListener('pointerup', stopPrimaryResize);

function startSidebarResize(event) {
  const content = event.target.closest('.sidebar-content');
  if (!content) return;
  const rect = content.getBoundingClientRect();
  sidebarResize = { top: rect.top, height: rect.height };
  document.body.classList.add('is-resizing-sidebar');
  event.preventDefault();
}

function updateSidebarResize(event) {
  if (!sidebarResize) return;
  const y = Math.max(0, Math.min(sidebarResize.height, event.clientY - sidebarResize.top));
  const percent = Math.round((y / sidebarResize.height) * 100);
  state.sidebarSplit = Math.max(30, Math.min(82, percent));
  const content = document.querySelector('.sidebar-content');
  if (content) content.style.setProperty('--tree-ratio', state.sidebarSplit);
}

function stopSidebarResize() {
  if (!sidebarResize) return;
  sidebarResize = null;
  document.body.classList.remove('is-resizing-sidebar');
}

document.addEventListener('pointermove', updateSidebarResize);
document.addEventListener('pointerup', stopSidebarResize);

function moveSidebarSelection(delta) {
  const items = [...document.querySelectorAll('.primary-sidebar [data-action="select"]')];
  if (!items.length) return;
  const currentId = activeEditorGroup()?.nodeId;
  let index = items.findIndex((item) => item.dataset.id === currentId);
  if (index < 0) index = 0;
  const next = Math.max(0, Math.min(items.length - 1, index + delta));
  const nextId = items[next]?.dataset.id;
  if (nextId) selectNode(nextId);
}

document.addEventListener("keydown", (event) => {
  const inSidebar = document.activeElement?.closest?.('.primary-sidebar');
  if (!inSidebar) return;
  if (event.key === 'ArrowDown') { event.preventDefault(); moveSidebarSelection(1); }
  if (event.key === 'ArrowUp') { event.preventDefault(); moveSidebarSelection(-1); }
});

render();
