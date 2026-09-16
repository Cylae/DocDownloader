pub const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>DocDownloader — Offline Document Reconstruction</title>
  <style>
    :root {
      --bg: #0d1117;
      --card-bg: #161b22;
      --border: #30363d;
      --text: #c9d1d9;
      --text-muted: #8b949e;
      --accent: #58a6ff;
      --accent-hover: #1f6feb;
      --success: #2ea043;
      --danger: #f85149;
      --radius: 8px;
    }
    @media (prefers-color-scheme: light) {
      :root {
        --bg: #f6f8fa;
        --card-bg: #ffffff;
        --border: #d0d7de;
        --text: #24292f;
        --text-muted: #57606a;
        --accent: #0969da;
        --accent-hover: #0550ae;
      }
    }
    * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
    body { background-color: var(--bg); color: var(--text); padding: 2rem 1rem; display: flex; justify-content: center; }
    .container { max-width: 720px; width: 100%; display: flex; flex-direction: column; gap: 1.5rem; }
    header { text-align: center; }
    header h1 { font-size: 1.75rem; margin-bottom: 0.5rem; }
    header p { color: var(--text-muted); font-size: 0.95rem; }
    .card { background: var(--card-bg); border: 1px solid var(--border); border-radius: var(--radius); padding: 1.5rem; box-shadow: 0 4px 12px rgba(0,0,0,0.1); }
    .form-group { display: flex; gap: 0.5rem; margin-bottom: 1rem; }
    input[type="text"] { flex: 1; padding: 0.75rem 1rem; border: 1px solid var(--border); border-radius: var(--radius); background: transparent; color: var(--text); font-size: 1rem; outline: none; }
    input[type="text"]:focus { border-color: var(--accent); }
    button { padding: 0.75rem 1.25rem; border: none; border-radius: var(--radius); background: var(--accent); color: #fff; font-weight: 600; cursor: pointer; transition: background 0.2s; }
    button:hover { background: var(--accent-hover); }
    button:disabled { opacity: 0.6; cursor: not-allowed; }
    .secondary-btn { background: transparent; border: 1px solid var(--border); color: var(--text); }
    .secondary-btn:hover { background: var(--border); }
    .danger-btn { background: var(--danger); }
    .preview-box { display: none; margin-top: 1rem; padding-top: 1rem; border-top: 1px solid var(--border); }
    .preview-content { display: flex; gap: 1rem; align-items: flex-start; }
    .preview-thumb { width: 100px; height: 140px; object-fit: cover; border-radius: 4px; border: 1px solid var(--border); background: #000; }
    .preview-details h3 { font-size: 1.1rem; margin-bottom: 0.25rem; }
    .preview-meta { font-size: 0.9rem; color: var(--text-muted); margin-bottom: 0.5rem; }
    .progress-section { display: none; margin-top: 1.5rem; }
    .progress-bar-bg { width: 100%; height: 10px; background: var(--border); border-radius: 5px; overflow: hidden; margin: 0.5rem 0; }
    .progress-bar-fill { height: 100%; width: 0%; background: var(--accent); transition: width 0.3s ease; }
    .status-text { font-size: 0.9rem; color: var(--text-muted); display: flex; justify-content: space-between; }
    .alert { padding: 0.75rem 1rem; border-radius: var(--radius); font-size: 0.9rem; margin-top: 1rem; display: none; }
    .alert-error { background: rgba(248, 81, 73, 0.15); border: 1px solid var(--danger); color: var(--danger); }
    .alert-success { background: rgba(46, 160, 67, 0.15); border: 1px solid var(--success); color: var(--success); }
  </style>
</head>
<body>
  <div class="container">
    <header>
      <h1>DocDownloader</h1>
      <p>High-Performance Offline Publication Reconstruction</p>
    </header>

    <div class="card">
      <div class="form-group">
        <input type="text" id="urlInput" placeholder="Paste publication URL (Calaméo, Issuu, SlideShare, Scribd)..." autofocus />
        <button id="inspectBtn" onclick="inspectUrl()">Inspect</button>
      </div>

      <div id="errorBox" class="alert alert-error"></div>
      <div id="successBox" class="alert alert-success"></div>

      <div id="previewBox" class="preview-box">
        <div class="preview-content">
          <img id="thumbImg" class="preview-thumb" src="" alt="Thumbnail" />
          <div class="preview-details">
            <h3 id="pubTitle"></h3>
            <p id="pubAuthor" class="preview-meta"></p>
            <p id="pubPages" class="preview-meta"></p>
            <p id="pubQuality" class="preview-meta">Target: Best Legitimate Reader Quality</p>
            <button id="downloadBtn" onclick="startDownload()" style="margin-top: 0.5rem;">Download Offline PDF</button>
          </div>
        </div>
      </div>

      <div id="progressSection" class="progress-section">
        <div class="status-text">
          <span id="stageLabel">Downloading pages...</span>
          <span id="percentLabel">0%</span>
        </div>
        <div class="progress-bar-bg">
          <div id="progressFill" class="progress-bar-fill"></div>
        </div>
        <div style="margin-top: 0.75rem; display: flex; justify-content: flex-end; gap: 0.5rem;">
          <button id="cancelBtn" class="secondary-btn danger-btn" onclick="cancelJob()">Cancel</button>
          <a id="downloadPdfLink" style="display: none;" href="#" download>
            <button class="secondary-btn" style="background: var(--success); color: #fff; border: none;">Save PDF File</button>
          </a>
        </div>
      </div>
    </div>
  </div>

  <script>
    let currentJobId = null;
    let eventSource = null;

    async function inspectUrl() {
      const url = document.getElementById('urlInput').value.trim();
      if (!url) return;

      hideAlerts();
      const inspectBtn = document.getElementById('inspectBtn');
      inspectBtn.disabled = true;
      inspectBtn.innerText = 'Detecting...';

      try {
        const resp = await fetch('/api/inspect', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ url })
        });
        const data = await resp.json();
        if (!resp.ok) {
          throw new Error(data.error || 'Inspection failed');
        }

        document.getElementById('pubTitle').innerText = data.title;
        document.getElementById('pubAuthor').innerText = data.author ? 'Author: ' + data.author : '';
        document.getElementById('pubPages').innerText = 'Pages: ' + data.page_count;
        if (data.thumbnail_url) {
          document.getElementById('thumbImg').src = data.thumbnail_url;
          document.getElementById('thumbImg').style.display = 'block';
        } else {
          document.getElementById('thumbImg').style.display = 'none';
        }
        document.getElementById('previewBox').style.display = 'block';
      } catch (err) {
        showError(err.message);
      } finally {
        inspectBtn.disabled = false;
        inspectBtn.innerText = 'Inspect';
      }
    }

    async function startDownload() {
      const url = document.getElementById('urlInput').value.trim();
      if (!url) return;

      hideAlerts();
      document.getElementById('downloadBtn').disabled = true;
      document.getElementById('progressSection').style.display = 'block';
      document.getElementById('progressFill').style.width = '0%';
      document.getElementById('percentLabel').innerText = '0%';
      document.getElementById('stageLabel').innerText = 'Initiating download...';
      document.getElementById('downloadPdfLink').style.display = 'none';

      try {
        const resp = await fetch('/api/download', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ url })
        });
        const data = await resp.json();
        if (!resp.ok) {
          throw new Error(data.error || 'Download failed');
        }

        currentJobId = data.job_id;
        listenProgress(currentJobId);
      } catch (err) {
        showError(err.message);
        document.getElementById('downloadBtn').disabled = false;
      }
    }

    function listenProgress(jobId) {
      if (eventSource) eventSource.close();
      eventSource = new EventSource('/api/jobs/' + jobId + '/events');

      eventSource.onmessage = function(event) {
        const update = JSON.parse(event.data);
        if (update.stage) {
          document.getElementById('stageLabel').innerText = update.stage;
        }
        if (update.total_pages && update.completed_pages !== undefined) {
          const pct = Math.round((update.completed_pages / update.total_pages) * 100);
          document.getElementById('progressFill').style.width = pct + '%';
          document.getElementById('percentLabel').innerText = pct + '%';
        }
        if (update.status === 'Completed') {
          eventSource.close();
          document.getElementById('stageLabel').innerText = 'Completed!';
          document.getElementById('progressFill').style.width = '100%';
          document.getElementById('percentLabel').innerText = '100%';
          const dlLink = document.getElementById('downloadPdfLink');
          dlLink.href = '/api/jobs/' + jobId + '/file';
          dlLink.style.display = 'inline-block';
          document.getElementById('downloadBtn').disabled = false;
          showSuccess('Document successfully reconstructed and validated!');
        } else if (update.status === 'Failed' || update.status === 'Cancelled') {
          eventSource.close();
          document.getElementById('downloadBtn').disabled = false;
          showError(update.error || update.status);
        }
      };

      eventSource.onerror = function() {
        eventSource.close();
      };
    }

    async function cancelJob() {
      if (!currentJobId) return;
      try {
        await fetch('/api/jobs/' + currentJobId + '/cancel', { method: 'POST' });
        if (eventSource) eventSource.close();
        document.getElementById('stageLabel').innerText = 'Cancelled';
        document.getElementById('downloadBtn').disabled = false;
      } catch (e) {
        console.error(e);
      }
    }

    function showError(msg) {
      const box = document.getElementById('errorBox');
      box.innerText = msg;
      box.style.display = 'block';
    }

    function showSuccess(msg) {
      const box = document.getElementById('successBox');
      box.innerText = msg;
      box.style.display = 'block';
    }

    function hideAlerts() {
      document.getElementById('errorBox').style.display = 'none';
      document.getElementById('successBox').style.display = 'none';
    }
  </script>
</body>
</html>
"##;
