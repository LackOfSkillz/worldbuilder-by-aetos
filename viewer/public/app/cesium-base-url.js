// Cesium resolves every worker, asset and widget image relative to this.
// It must be set BEFORE Cesium.js is evaluated. There is no remote fallback:
// buildModuleUrl() only ever joins against this base.
//
// This is a separate file, and not an inline <script>, only so the page can be served
// under `script-src 'self'` with no `'unsafe-inline'` (Task 6). A classic <script src>
// with no `defer`/`async` still blocks and still runs in document order, so this is
// guaranteed to have run before /vendor/cesium/Cesium.js is evaluated.
window.CESIUM_BASE_URL = "/vendor/cesium/";
