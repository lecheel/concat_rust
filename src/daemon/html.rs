pub const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en" class="h-full bg-slate-950 text-slate-100">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Concat Rust — Daemon Dashboard</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <style>
        ::-webkit-scrollbar {
            width: 6px;
            height: 6px;
        }
        ::-webkit-scrollbar-track {
            background: #0f172a;
        }
        ::-webkit-scrollbar-thumb {
            background: #334155;
            border-radius: 3px;
        }
        ::-webkit-scrollbar-thumb:hover {
            background: #475569;
        }
    </style>
</head>
<body class="h-full flex flex-col font-sans overflow-hidden">
    <!-- Top Nav Bar -->
    <header class="bg-slate-900 border-b border-slate-800 px-6 py-4 flex items-center justify-between shadow-md shrink-0">
        <div class="flex items-center space-x-3">
            <span class="text-2xl">⚡</span>
            <div>
                <h1 class="text-lg font-bold tracking-tight text-white">Concat Rust</h1>
                <p class="text-xs text-slate-400">Daemon Backend Dashboard</p>
            </div>
        </div>
        <div class="flex items-center space-x-4">
            <div id="connection-status" class="flex items-center space-x-2 text-xs font-semibold bg-emerald-950/50 text-emerald-400 border border-emerald-800/60 px-2.5 py-1 rounded-full">
                <span class="h-2 w-2 rounded-full bg-emerald-400 animate-pulse"></span>
                <span>Daemon Active</span>
            </div>
            <button onclick="syncAll()" class="bg-indigo-600 hover:bg-indigo-500 text-white font-medium text-xs px-3 py-1.5 rounded transition duration-150 shadow flex items-center space-x-1.5">
                <span id="sync-spinner" class="hidden animate-spin h-3.5 w-3.5 border-2 border-white border-t-transparent rounded-full"></span>
                <span>Sync & Reindex All</span>
            </button>
        </div>
    </header>

    <!-- Main Workspace -->
    <main class="flex-1 flex overflow-hidden">
        <!-- Sidebar (Tabs: Repos, Catalog, Activity Logs, Repo Stats) -->
        <div class="w-96 border-r border-slate-800 bg-slate-900/40 flex flex-col overflow-hidden shrink-0">
            <!-- Tabs Header -->
            <div class="flex border-b border-slate-800 shrink-0">
                <button onclick="switchTab('repos')" id="tab-btn-repos" class="flex-1 py-2 text-[11px] font-semibold border-b-2 border-indigo-500 text-white transition">Repos</button>
                <button onclick="switchTab('catalog')" id="tab-btn-catalog" class="flex-1 py-2 text-[11px] font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition">Catalog</button>
                <button onclick="switchTab('logs')" id="tab-btn-logs" class="flex-1 py-2 text-[11px] font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition">Logs</button>
                <button onclick="switchTab('stats')" id="tab-btn-stats" class="flex-1 py-2 text-[11px] font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition">Stats</button>
            </div>

            <!-- Tab Content Area -->
            <div class="flex-1 overflow-y-auto p-4 space-y-4">
                <!-- REPOS TAB -->
                <div id="tab-content-repos" class="space-y-4">
                    <div class="bg-slate-900/80 rounded-lg p-4 border border-slate-800 space-y-3 shadow-inner">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400">Add Git Repository</h3>
                        <form id="add-repo-form" onsubmit="handleAddRepo(event)" class="space-y-2.5">
                            <div>
                                <label class="block text-[10px] font-bold text-slate-400 uppercase mb-1">Repo ID (slug)</label>
                                <input type="text" id="repo-id" required placeholder="e.g. core-api" class="w-full bg-slate-950 border border-slate-800 rounded px-2.5 py-1.5 text-xs focus:outline-none focus:border-indigo-500 text-slate-200">
                            </div>
                            <div>
                                <label class="block text-[10px] font-bold text-slate-400 uppercase mb-1">Source Path (Absolute)</label>
                                <input type="text" id="repo-path" required placeholder="e.g. /Users/dev/projects/core-api" class="w-full bg-slate-950 border border-slate-800 rounded px-2.5 py-1.5 text-xs focus:outline-none focus:border-indigo-500 text-slate-200">
                            </div>
                            <button type="submit" class="w-full bg-indigo-600/20 hover:bg-indigo-600/30 text-indigo-400 border border-indigo-500/30 font-medium text-xs py-1.5 rounded transition">
                                Register Repo
                            </button>
                        </form>
                    </div>

                    <div class="space-y-3">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400 px-1">Registered Mirror Repos</h3>
                        <div id="repos-list" class="space-y-2"></div>
                    </div>
                </div>

                <!-- CATALOG TAB -->
                <div id="tab-content-catalog" class="hidden space-y-3">
                    <div class="sticky top-0 bg-slate-950/90 py-1 shrink-0">
                        <input type="text" id="catalog-search" oninput="filterCatalog()" placeholder="Search catalog paths..." class="w-full bg-slate-950 border border-slate-800 rounded px-3 py-2 text-xs focus:outline-none focus:border-indigo-500 text-slate-200">
                    </div>
                    <div id="catalog-list" class="space-y-1.5"></div>
                </div>

                <!-- ACTIVITY LOGS TAB -->
                <div id="tab-content-logs" class="hidden space-y-3">
                    <div class="flex items-center justify-between px-1">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400">Daemon Activity Logs</h3>
                        <span class="text-[9px] bg-indigo-950 text-indigo-400 border border-indigo-900 rounded px-1.5 py-0.5 font-semibold">Auto-refresh (2s)</span>
                    </div>
                    <div id="logs-list" class="space-y-2"></div>
                </div>

                <!-- STATS TAB -->
                <div id="tab-content-stats" class="hidden space-y-3">
                    <div class="flex items-center justify-between px-1">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-slate-400">Usage Stats</h3>
                        <button onclick="loadStats()" class="text-xs text-indigo-400 hover:text-indigo-300 font-semibold flex items-center space-x-1">
                            <span>🔄</span><span>Refresh</span>
                        </button>
                    </div>
                    <div id="stats-list" class="space-y-3"></div>
                </div>
            </div>
        </div>

        <!-- Dashboard Content Pane (Right side) -->
        <div class="flex-1 flex flex-col overflow-hidden bg-slate-950">
            <!-- Search / Inspector bar -->
            <div class="bg-slate-900/60 border-b border-slate-800/80 px-6 py-3 shrink-0 flex items-center justify-between">
                <div class="flex items-center space-x-2 w-full max-w-lg">
                    <span class="text-xs font-bold text-slate-400 uppercase tracking-wider shrink-0">Hash Inspector:</span>
                    <input type="text" id="hash-search" placeholder="Type HASH prefix (e.g. 5d1f) + Enter..." onkeydown="handleHashSearch(event)" class="flex-1 bg-slate-950 border border-slate-800 rounded-md px-3 py-1.5 text-xs font-mono text-slate-200 focus:outline-none focus:border-indigo-500">
                    <button onclick="inspectHash()" class="bg-slate-800 hover:bg-slate-700 border border-slate-700/60 text-slate-300 px-3 py-1.5 text-xs rounded transition font-medium shrink-0">Inspect</button>
                </div>
                <div class="text-xs text-slate-500 font-medium font-mono" id="right-pane-stats">
                    Select a file or lookup a hash context to inspect
                </div>
            </div>

            <!-- Inspection Output Viewport -->
            <div class="flex-1 flex overflow-hidden">
                <div class="flex-1 flex flex-col overflow-hidden p-6 space-y-4">
                    <div id="metadata-header" class="hidden shrink-0 bg-slate-900/40 border border-slate-800/80 rounded-lg p-4 flex items-center justify-between">
                        <div class="space-y-1">
                            <div class="flex items-center space-x-2">
                                <span id="meta-icon" class="text-base">📄</span>
                                <h2 id="meta-title" class="text-sm font-bold font-mono text-indigo-400">path/to/file.rs</h2>
                            </div>
                            <p id="meta-subtitle" class="text-xs text-slate-400">File stats: ...</p>
                        </div>
                        <div class="flex space-x-2">
                            <button onclick="copyCurrentCode()" class="bg-slate-800 hover:bg-slate-700 text-slate-300 text-xs px-3 py-1.5 rounded transition border border-slate-700">Copy Code</button>
                        </div>
                    </div>

                    <!-- Code block viewer -->
                    <div class="flex-1 bg-slate-900/20 border border-slate-800 rounded-lg overflow-hidden flex flex-col">
                        <div class="bg-slate-900/60 border-b border-slate-800 px-4 py-2 flex items-center justify-between text-xs text-slate-400 shrink-0 font-medium">
                            <span id="code-viewer-title">No content selected</span>
                            <span id="code-viewer-size">0 bytes</span>
                        </div>
                        <div class="flex-1 overflow-auto relative p-4 bg-slate-950">
                            <pre id="code-display" class="text-xs font-mono text-slate-300 leading-relaxed overflow-x-auto select-text">Select a source file from the catalog or paste a code hash to view implementation details.</pre>
                        </div>
                    </div>
                </div>

                <!-- Right panel for File Hashes -->
                <div id="file-hashes-panel" class="hidden w-80 border-l border-slate-800 bg-slate-900/20 flex flex-col overflow-hidden shrink-0">
                    <div class="p-4 border-b border-slate-800 shrink-0 bg-slate-900/40">
                        <h3 class="text-xs font-bold uppercase tracking-wider text-indigo-400">Extracted AST Code Bodies</h3>
                        <p class="text-[10px] text-slate-500 mt-1">Rust elements with unique stable hashes. Click a hash to isolate its implementation.</p>
                    </div>
                    <div id="file-hashes-list" class="flex-1 overflow-y-auto p-4 space-y-2"></div>
                </div>
            </div>
        </div>
    </main>

    <!-- Message Toasts -->
    <div id="toast-container" class="fixed bottom-4 right-4 z-50 space-y-2"></div>

    <script>
        let currentTab = 'repos';
        let loadedCatalogData = [];
        let logInterval = null;

        function escapeHtml(text) {
            return text
                .replace(/&/g, "&amp;")
                .replace(/</g, "&lt;")
                .replace(/>/g, "&gt;")
                .replace(/"/g, "&quot;")
                .replace(/'/g, "&#039;");
        }

        function showToast(message, isError = false) {
            const container = document.getElementById('toast-container');
            const toast = document.createElement('div');
            toast.className = `px-4 py-2.5 rounded-lg border shadow-lg text-xs font-medium transition duration-300 transform translate-y-2 opacity-0 flex items-center space-x-2 ${
                isError 
                ? 'bg-red-950 border-red-800 text-red-300' 
                : 'bg-slate-900 border-slate-700 text-indigo-300'
            }`;
            toast.innerHTML = `<span>${isError ? '❌' : '✅'}</span><span>${message}</span>`;
            container.appendChild(toast);
            
            setTimeout(() => toast.classList.remove('translate-y-2', 'opacity-0'), 10);
            setTimeout(() => {
                toast.classList.add('opacity-0', 'translate-y-2');
                setTimeout(() => toast.remove(), 300);
            }, 4000);
        }

        function getClientLabel(ua) {
            if (!ua) return 'Unknown Client';
            if (ua.includes('reqwest') || ua.includes('concat_rust_cli')) {
                return '💻 Concat CLI';
            }
            if (ua.includes('Mozilla') || ua.includes('Chrome') || ua.includes('Safari')) {
                return '🌐 Web Dashboard';
            }
            return ua;
        }

        function switchTab(tab) {
            currentTab = tab;
            
            if (tab === 'logs') {
                startLogPolling();
            } else {
                stopLogPolling();
            }

            document.getElementById('tab-btn-repos').className = tab === 'repos' 
                ? 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-indigo-500 text-white transition' 
                : 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition';
            
            document.getElementById('tab-btn-catalog').className = tab === 'catalog' 
                ? 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-indigo-500 text-white transition' 
                : 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition';

            document.getElementById('tab-btn-logs').className = tab === 'logs' 
                ? 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-indigo-500 text-white transition' 
                : 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition';

            document.getElementById('tab-btn-stats').className = tab === 'stats' 
                ? 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-indigo-500 text-white transition' 
                : 'flex-1 py-2 text-[11px] font-semibold border-b-2 border-transparent text-slate-400 hover:text-slate-200 transition';

            if (tab === 'repos') {
                document.getElementById('tab-content-repos').classList.remove('hidden');
                document.getElementById('tab-content-catalog').classList.add('hidden');
                document.getElementById('tab-content-logs').classList.add('hidden');
                document.getElementById('tab-content-stats').classList.add('hidden');
            } else if (tab === 'catalog') {
                document.getElementById('tab-content-repos').classList.add('hidden');
                document.getElementById('tab-content-catalog').classList.remove('hidden');
                document.getElementById('tab-content-logs').classList.add('hidden');
                document.getElementById('tab-content-stats').classList.add('hidden');
                loadCatalog();
            } else if (tab === 'logs') {
                document.getElementById('tab-content-repos').classList.add('hidden');
                document.getElementById('tab-content-catalog').classList.add('hidden');
                document.getElementById('tab-content-logs').classList.remove('hidden');
                document.getElementById('tab-content-stats').classList.add('hidden');
            } else if (tab === 'stats') {
                document.getElementById('tab-content-repos').classList.add('hidden');
                document.getElementById('tab-content-catalog').classList.add('hidden');
                document.getElementById('tab-content-logs').classList.add('hidden');
                document.getElementById('tab-content-stats').classList.remove('hidden');
                loadStats();
            }
        }

        async function loadLogs() {
            try {
                const res = await fetch('/logs');
                if (!res.ok) throw new Error('Logs offline');
                const data = await res.json();
                const logs = data.logs || [];
                const container = document.getElementById('logs-list');
                container.innerHTML = '';

                if (logs.length === 0) {
                    container.innerHTML = `<div class="text-center py-6 text-xs text-slate-500 font-sans">No client connections received yet. Run terminal commands to trigger request logs.</div>`;
                    return;
                }

                logs.reverse().forEach(log => {
                    const timeStr = new Date(log.timestamp * 1000).toLocaleTimeString();
                    const isSuccess = log.status >= 200 && log.status < 300;
                    const statusColor = isSuccess ? 'text-emerald-400 bg-emerald-950/40 border-emerald-800/40' : 'text-red-400 bg-red-950/40 border-red-800/40';
                    const clientLabel = getClientLabel(log.user_agent);

                    const item = document.createElement('div');
                    item.className = 'p-2 bg-slate-900 border border-slate-800/60 rounded flex flex-col space-y-1.5 transition';
                    item.innerHTML = `
                        <div class="flex justify-between items-center text-[10px] text-slate-500 font-sans">
                            <span>${timeStr}</span>
                            <span class="truncate font-semibold text-slate-400" title="${log.user_agent}">${clientLabel}</span>
                        </div>
                        <div class="flex items-center space-x-1.5 pt-0.5">
                            <span class="px-1.5 py-0.5 rounded text-[10px] border font-bold ${statusColor}">${log.status}</span>
                            <span class="text-slate-300 truncate font-mono text-[10px]" title="${log.path}">${log.method} ${log.path}</span>
                        </div>
                    `;
                    container.appendChild(item);
                });
            } catch (err) {
                console.error('Log sync failed:', err);
            }
        }

        async function loadStats() {
            try {
                const res = await fetch('/stats');
                if (!res.ok) throw new Error('Stats offline');
                const statsList = await res.json();
                const container = document.getElementById('stats-list');
                container.innerHTML = '';

                if (statsList.length === 0) {
                    container.innerHTML = `<div class="text-center py-6 text-xs text-slate-500 font-sans">No stats found. Register repos and fetch files or hashes to generate statistics.</div>`;
                    return;
                }

                statsList.forEach(repo => {
                    const repoBox = document.createElement('div');
                    repoBox.className = 'bg-slate-900/60 border border-slate-800 rounded-lg p-3 space-y-3 shadow';
                    
                    let fileRows = '';
                    if (repo.top_files.length === 0) {
                        fileRows = '<div class="text-[10px] text-slate-500 italic">No file accesses recorded yet.</div>';
                    } else {
                        repo.top_files.forEach(f => {
                            fileRows += `
                                <div onclick="viewFile('${f.full_path}')" class="w-full text-left bg-slate-950/40 hover:bg-slate-800 border border-slate-800/40 hover:border-slate-700/60 p-1.5 rounded text-[10px] transition font-mono flex items-center justify-between cursor-pointer group">
                                    <span class="truncate pr-1 text-slate-300 group-hover:text-white" title="${f.full_path}">${f.filepath}</span>
                                    <div class="flex items-center space-x-1.5 shrink-0">
                                        <span class="text-[9px] text-slate-500 font-semibold">${f.loc} LOC</span>
                                        <span class="bg-indigo-950/60 text-indigo-400 px-1.5 py-0.5 rounded font-bold">${f.requests} hits</span>
                                    </div>
                                </div>
                            `;
                        });
                    }

                    let hashRows = '';
                    if (repo.top_hashes.length === 0) {
                        hashRows = '<div class="text-[10px] text-slate-500 italic">No hash accesses recorded yet.</div>';
                    } else {
                        repo.top_hashes.forEach(h => {
                            hashRows += `
                                <div onclick="loadSpecificHash('${h.hash}')" class="w-full text-left bg-slate-950/40 hover:bg-slate-800 border border-slate-800/40 hover:border-slate-700/60 p-1.5 rounded text-[10px] transition font-mono flex items-center justify-between cursor-pointer group">
                                    <span class="text-indigo-400 font-bold group-hover:text-indigo-300 shrink-0"># ${h.hash}</span>
                                    <span class="truncate px-1.5 text-slate-400" title="${h.filepath}">${h.filepath}</span>
                                    <span class="bg-slate-900 text-slate-400 px-1.5 py-0.5 rounded shrink-0">${h.requests} hits</span>
                                </div>
                            `;
                        });
                    }

                    repoBox.innerHTML = `
                        <div class="border-b border-slate-800/80 pb-2 flex items-center justify-between">
                            <span class="font-bold text-xs text-indigo-400 font-mono">📦 ${repo.repo_id}</span>
                            <span class="text-[9px] bg-slate-800 text-slate-300 px-1.5 py-0.5 rounded font-mono">${repo.total_files} files | ${repo.total_loc} LOC</span>
                        </div>
                        <div class="grid grid-cols-2 gap-2 text-[10px] text-slate-400 font-medium">
                            <div class="bg-slate-950/30 border border-slate-800/50 p-2 rounded">
                                <span class="block text-[8px] uppercase tracking-wider text-slate-500 mb-1 font-bold">Total File Hits</span>
                                <span class="text-sm font-bold text-slate-200 font-mono">${repo.total_file_requests}</span>
                            </div>
                            <div class="bg-slate-950/30 border border-slate-800/50 p-2 rounded">
                                <span class="block text-[8px] uppercase tracking-wider text-slate-500 mb-1 font-bold">Total Disk Size</span>
                                <span class="text-sm font-bold text-slate-200 font-mono">${(repo.total_bytes / 1024).toFixed(1)} KB</span>
                            </div>
                        </div>
                        <div class="space-y-1.5">
                            <h4 class="text-[9px] font-bold uppercase tracking-wider text-slate-400">🔥 Top Requested Files</h4>
                            <div class="space-y-1">${fileRows}</div>
                        </div>
                        <div class="space-y-1.5 pt-1">
                            <h4 class="text-[9px] font-bold uppercase tracking-wider text-slate-400">🔑 Top Requested Hashes</h4>
                            <div class="space-y-1">${hashRows}</div>
                        </div>
                    `;
                    container.appendChild(repoBox);
                });
            } catch (err) {
                console.error('Stats load failed:', err);
            }
        }

        function startLogPolling() {
            if (!logInterval) {
                loadLogs();
                logInterval = setInterval(loadLogs, 2000);
            }
        }

        function stopLogPolling() {
            if (logInterval) {
                clearInterval(logInterval);
                logInterval = null;
            }
        }

        async function loadRepos() {
            try {
                const res = await fetch('/repos');
                if (!res.ok) throw new Error(`HTTP error! status: ${res.status}`);
                const repos = await res.json();
                const list = document.getElementById('repos-list');
                list.innerHTML = '';
                
                if (repos.length === 0) {
                    list.innerHTML = `<div class="text-center py-6 text-xs text-slate-500">No repositories registered yet. Use the form above.</div>`;
                    return;
                }

                repos.forEach(repo => {
                    const dateStr = repo.last_sync 
                        ? new Date(repo.last_sync * 1000).toLocaleString() 
                        : 'Never Synced';
                    const activeDot = repo.active 
                        ? '<span class="h-2 w-2 rounded-full bg-emerald-500"></span>' 
                        : '<span class="h-2 w-2 rounded-full bg-slate-500"></span>';
                    
                    const item = document.createElement('div');
                    item.className = 'bg-slate-900 border border-slate-800 rounded-lg p-3 space-y-2 hover:border-slate-700 transition relative group';
                    item.innerHTML = `
                        <div class="flex items-center justify-between">
                            <div class="flex items-center space-x-2">
                                ${activeDot}
                                <span class="font-bold text-xs text-white font-mono">${repo.id}</span>
                                <span class="text-[10px] bg-indigo-950 text-indigo-400 border border-indigo-900 rounded px-1.5 py-0.5 font-medium font-mono">${repo.git_branch || 'detached'}</span>
                            </div>
                            <button onclick="deleteRepo('${repo.id}')" class="text-slate-500 hover:text-red-400 text-xs p-1 rounded hover:bg-slate-800 transition">🗑️</button>
                        </div>
                        <div class="text-[11px] text-slate-400 font-mono truncate" title="${repo.source_path}">
                            Path: ${repo.source_path}
                        </div>
                        <div class="flex justify-between items-center text-[10px] text-slate-500 border-t border-slate-800/60 pt-2 font-mono">
                            <span>Files: ${repo.file_count || 0}</span>
                            <span>Synced: ${dateStr}</span>
                        </div>
                    `;
                    list.appendChild(item);
                });
            } catch (err) {
                console.error(err);
                showToast('Failed to load repositories', true);
            }
        }

        async function handleAddRepo(e) {
            e.preventDefault();
            const id = document.getElementById('repo-id').value.trim();
            const path = document.getElementById('repo-path').value.trim();
            if (!id || !path) return;

            try {
                const res = await fetch('/repos', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ id, source_path: path })
                });
                const msg = await res.text();
                if (!res.ok) throw new Error(msg);
                
                showToast(msg);
                document.getElementById('add-repo-form').reset();
                loadRepos();
            } catch (err) {
                showToast(err.message || 'Failed to register repo', true);
            }
        }

        async function deleteRepo(id) {
            if (!confirm(`Remove repo '${id}'?`)) return;
            try {
                const res = await fetch(`/repos/${id}`, { method: 'DELETE' });
                const msg = await res.text();
                if (!res.ok) throw new Error(msg);
                
                showToast(msg);
                loadRepos();
            } catch (err) {
                showToast(err.message || 'Failed to delete repo', true);
            }
        }

        async function syncAll() {
            const spinner = document.getElementById('sync-spinner');
            spinner.classList.remove('hidden');
            try {
                const res = await fetch('/sync', { method: 'POST' });
                const msg = await res.text();
                if (!res.ok) throw new Error(msg);
                
                showToast(msg);
                loadRepos();
                if (currentTab === 'catalog') loadCatalog();
            } catch (err) {
                showToast(err.message || 'Sync failed', true);
            } finally {
                spinner.classList.add('hidden');
            }
        }

        async function loadCatalog() {
            try {
                const res = await fetch('/catalog');
                if (!res.ok) throw new Error('Failed to fetch catalog');
                const data = await res.json();
                loadedCatalogData = data.files || [];
                filterCatalog();
            } catch (err) {
                showToast('Failed to load file catalog', true);
            }
        }

        function filterCatalog() {
            const query = document.getElementById('catalog-search').value.toLowerCase();
            const list = document.getElementById('catalog-list');
            list.innerHTML = '';

            const filtered = loadedCatalogData.filter(file => 
                file.filepath.toLowerCase().includes(query)
            );

            if (filtered.length === 0) {
                list.innerHTML = `<div class="text-center py-6 text-xs text-slate-500">No matching files.</div>`;
                return;
            }

            filtered.forEach(file => {
                const item = document.createElement('button');
                item.className = 'w-full text-left bg-slate-900 hover:bg-slate-800 text-slate-300 hover:text-white px-3 py-2 rounded text-xs transition font-mono border border-slate-800/40 flex items-center justify-between group';
                item.onclick = () => viewFile(file.filepath);
                
                const fileIcon = file.filepath.endsWith('.rs') ? '🦀' : file.filepath.endsWith('.go') ? '🐹' : '📄';
                item.innerHTML = `
                    <div class="truncate pr-2 flex items-center space-x-1.5">
                        <span class="text-[10px]">${fileIcon}</span>
                        <span class="truncate" title="${file.filepath}">${file.filepath}</span>
                    </div>
                    <span class="text-[9px] text-slate-500 group-hover:text-slate-400 font-semibold bg-slate-950 px-1.5 py-0.5 rounded shrink-0">${file.loc} LOC</span>
                `;
                list.appendChild(item);
            });
        }

        async function viewFile(filepath) {
            try {
                const displayHeader = document.getElementById('metadata-header');
                displayHeader.classList.remove('hidden');
                document.getElementById('meta-icon').innerText = filepath.endsWith('.rs') ? '🦀' : filepath.endsWith('.go') ? '🐹' : '📄';
                document.getElementById('meta-title').innerText = filepath;
                document.getElementById('meta-subtitle').innerText = 'Loading file contents...';

                const codeRes = await fetch(`/file/${filepath}`);
                if (!codeRes.ok) throw new Error('Could not fetch file contents.');
                const rawCode = await codeRes.text();

                let viewCode = rawCode;
                if (rawCode.startsWith('//--+ file:///')) {
                    const newlinePos = rawCode.indexOf('\n');
                    if (newlinePos !== -1) {
                        viewCode = rawCode.substring(newlinePos + 1);
                    }
                }

                document.getElementById('code-display').innerHTML = escapeHtml(viewCode);
                document.getElementById('code-viewer-title').innerText = filepath;
                document.getElementById('code-viewer-size').innerText = `${viewCode.length} bytes`;
                
                const hashesPanel = document.getElementById('file-hashes-panel');
                if (filepath.endsWith('.rs') || filepath.endsWith('.go')) {
                    hashesPanel.classList.remove('hidden');
                    const infoRes = await fetch(`/file-info/${filepath}`);
                    if (infoRes.ok) {
                        const info = await infoRes.json();
                        document.getElementById('meta-subtitle').innerText = `${info.loc} LOC | ${info.byte_size} bytes`;
                        renderFileHashes(info.body_hashes || []);
                    } else {
                        hashesPanel.classList.add('hidden');
                    }
                } else {
                    hashesPanel.classList.add('hidden');
                    document.getElementById('meta-subtitle').innerText = `${viewCode.split('\n').length} LOC | ${viewCode.length} bytes`;
                }
                
                document.getElementById('right-pane-stats').innerText = `Viewing: ${filepath}`;
            } catch (err) {
                showToast('Failed to open file', true);
            }
        }

        function renderFileHashes(hashes) {
            const container = document.getElementById('file-hashes-list');
            container.innerHTML = '';

            if (hashes.length === 0) {
                container.innerHTML = '<div class="text-slate-500 text-xs text-center py-4">No hash fragments.</div>';
                return;
            }

            hashes.forEach(item => {
                const btn = document.createElement('button');
                btn.className = 'w-full text-left bg-slate-950 hover:bg-indigo-950/20 hover:border-indigo-800/80 border border-slate-800 rounded p-2.5 space-y-1.5 transition block group';
                btn.onclick = () => loadSpecificHash(item.hash);
                btn.innerHTML = `
                    <div class="flex items-center justify-between">
                        <span class="font-mono text-[10px] text-indigo-400 font-bold group-hover:text-indigo-300"># ${item.hash}</span>
                        <span class="text-[9px] text-slate-500 font-bold bg-slate-900 px-1 rounded">${item.loc} LOC</span>
                    </div>
                `;
                container.appendChild(btn);
            });
        }

        async function loadSpecificHash(hash) {
            document.getElementById('hash-search').value = hash;
            inspectHash();
        }

        function handleHashSearch(e) {
            if (e.key === 'Enter') inspectHash();
        }

        async function inspectHash() {
            const query = document.getElementById('hash-search').value.trim();
            if (!query) return;

            try {
                const infoRes = await fetch(`/info/${query}`);
                if (!infoRes.ok) throw new Error('Hash info not found');
                const infos = await infoRes.json();

                const codeRes = await fetch(`/${query}`);
                if (!codeRes.ok) throw new Error('Could not fetch hash code body.');
                const bodyCodeRaw = await codeRes.text();

                let bodyCode = bodyCodeRaw;
                if (bodyCodeRaw.startsWith('//--+ file:///')) {
                    const newlinePos = bodyCodeRaw.indexOf('\n');
                    if (newlinePos !== -1) {
                        bodyCode = bodyCodeRaw.substring(newlinePos + 1);
                    }
                }

                document.getElementById('metadata-header').classList.remove('hidden');
                document.getElementById('meta-icon').innerText = '🔑';
                
                if (infos.length === 1) {
                    const info = infos[0];
                    document.getElementById('meta-title').innerText = `Hash: ${info.hash}`;
                    document.getElementById('meta-subtitle').innerText = `Source file: ${info.filepath} | Scope size: ${info.loc} LOC | ${info.byte_size} bytes`;
                    document.getElementById('code-viewer-title').innerText = `Scope context: ${info.filepath}`;
                } else {
                    document.getElementById('meta-title').innerText = `Hashes: ${query}`;
                    document.getElementById('meta-subtitle').innerText = `Multiple hash definitions found (${typeInfos.length})`;
                    document.getElementById('code-viewer-title').innerText = `Combined Context`;
                }

                document.getElementById('code-display').innerHTML = escapeHtml(bodyCode);
                document.getElementById('code-viewer-size').innerText = `${bodyCode.length} bytes`;
                document.getElementById('file-hashes-panel').classList.add('hidden');
                document.getElementById('right-pane-stats').innerText = `Inspecting: ${query}`;
            } catch (err) {
                showToast(err.message || 'Failed to inspect hash', true);
            }
        }

        function copyCurrentCode() {
            const codeText = document.getElementById('code-display').innerText;
            navigator.clipboard.writeText(codeText).then(() => {
                showToast('Code copied to clipboard!');
            }).catch(err => {
                showToast('Failed to copy code', true);
            });
        }

        window.onload = () => {
            loadRepos();
        };
    </script>
</body>
</html>"#;
