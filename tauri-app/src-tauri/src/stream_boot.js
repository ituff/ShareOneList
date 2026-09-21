// ShareOneList stream-capture boot script.
//
// Injected via Tauri `initialization_script_for_all_frames`, so it runs in
// every frame of every page loaded by the app webview — including the
// cross-origin SharePoint player iframes that Graph preview embeds.
//
// It is a faithful in-page port of the technique used by
// https://github.com/brendangooden/ms-teams-sharepoint-downloader (MIT):
// the SharePoint Stream player fetches a DASH `videomanifest` from the
// .svc.ms CDN with an `x-spopactoken` bearer header; segments live on
// sharepoint.com (manifest <BaseURL>) and are authenticated purely by the
// browser session's cookies. None of that is mintable from a desktop
// backend, so the download pipeline must run inside this page context.
//
// Protocol with the app frame (React, via postMessage, '*' targets — the
// messages carry no secrets beyond the short-lived capture itself):
//   up   SOL_CAPTURED {captureId}                      — manifest was seen; enables the UI button
//   up   SOL_PROGRESS {captureId, done, total, text}   — segment/mux progress
//   up   SOL_DONE {captureId}                          — bytes delivered to the loopback server
//   up   SOL_ERROR {captureId, message, isDrm}         — fatal pipeline error
//   down SOL_START {captureId, port, uploadToken}      — app frame hands back the loopback channel
//   down SOL_CANCEL {captureId}                        — user aborted
// SOL_* messages received from child frames are relayed upward; SOL_START /
// SOL_CANCEL are relayed downward until the frame holding that captureId
// picks them up.
(function () {
  'use strict';
  if (window.__SOL_STREAM_BOOT__) return;
  window.__SOL_STREAM_BOOT__ = true;

  var SHAREPOINT_HOST = /(^|\.)(sharepoint\.com|sharepoint\.cn|svc\.ms)$/i;
  if (!SHAREPOINT_HOST.test(location.hostname)) return;

  var capture = null; // { captureId, manifestUrl, spopactoken }
  var running = null; // { captureId, abort: AbortController }
  var spBearer = null; // Authorization header seen on /_api/v2.x player calls

  function hostOf(url) {
    try { return new URL(url).host; } catch (e) { return ''; }
  }

  // Segment hosts vary by tenant rollout: same-origin sharepoint.com URLs
  // are cookie-authed, but some manifests hand out .svc.ms CDN URLs (which
  // need the captured x-spopactoken bearer) or /_api/v2.x REST URLs (which
  // accept the player's SharePoint bearer). Attach whatever the URL needs.
  function segFetchInit(url, signal) {
    if (capture && capture.spopactoken && url.indexOf('svc.ms') !== -1) {
      return { signal: signal, headers: { 'x-spopactoken': capture.spopactoken } };
    }
    if (spBearer && url.indexOf('/_api/v') !== -1) {
      return { signal: signal, headers: { 'Authorization': spBearer } };
    }
    return { signal: signal };
  }

  // ── Fetch hook (adapted from intercept.js) ────────────────────────────────
  var nativeFetch = window.fetch ? window.fetch.bind(window) : null;
  if (nativeFetch) {
    window.fetch = function (input, init) {
      try {
        var url = typeof input === 'string' ? input : (input && input.url) || '';
        if (url.indexOf('videomanifest') !== -1 && !/tempauth/i.test(url)) {
          var trimmed = url;
          var dashIndex = trimmed.indexOf('index&format=dash');
          if (dashIndex !== -1) trimmed = trimmed.substring(0, dashIndex + 'index&format=dash'.length);
          var tok = extractHeader([input, init], 'x-spopactoken');
          capture = { captureId: String(Date.now()) + '-' + String(Math.random()).slice(2), manifestUrl: trimmed, spopactoken: tok };
          reportUp({ type: 'SOL_CAPTURED', captureId: capture.captureId });
        }
        // Capture the SharePoint Stream API bearer the player uses on
        // /_api/v2.x calls — needed when manifest segments point back at
        // sharepoint.com REST endpoints instead of plain cookie-authed files.
        if (/\/_api\/v[0-9.]+\//.test(url) && url.indexOf('sharepoint') !== -1) {
          var auth = extractHeader([input, init], 'authorization');
          if (auth && /^Bearer\s/i.test(auth)) spBearer = auth;
        }
      } catch (e) { /* best-effort capture */ }
      return nativeFetch(input, init);
    };
  }

  function extractHeader(args, headerName) {
    var key = headerName.toLowerCase();
    try {
      if (args[0] && typeof args[0] === 'object' && args[0].headers && typeof args[0].headers.get === 'function') {
        return args[0].headers.get(key);
      }
      var h = args[1] && args[1].headers;
      if (!h) return null;
      if (typeof Headers !== 'undefined' && h instanceof Headers) return h.get(key);
      if (Array.isArray(h)) {
        for (var i = 0; i < h.length; i++) {
          if (Array.isArray(h[i]) && h[i][0] && String(h[i][0]).toLowerCase() === key) return h[i][1];
        }
        return null;
      }
      var keys = Object.keys(h);
      for (var j = 0; j < keys.length; j++) {
        if (keys[j].toLowerCase() === key) return h[keys[j]];
      }
    } catch (e) { /* ignore */ }
    return null;
  }

  function reportUp(msg) {
    try { window.parent.postMessage(msg, '*'); } catch (e) { /* ignore */ }
  }
  function forwardDown(msg) {
    var frames = document.querySelectorAll('iframe');
    for (var i = 0; i < frames.length; i++) {
      try { frames[i].contentWindow.postMessage(msg, '*'); } catch (e) { /* ignore */ }
    }
  }

  window.addEventListener('message', function (ev) {
    var d = ev.data;
    if (!d || typeof d.type !== 'string' || d.type.indexOf('SOL_') !== 0) return;
    if (d.type === 'SOL_START' || d.type === 'SOL_CANCEL') {
      if (capture && d.captureId === capture.captureId) {
        if (d.type === 'SOL_START') runDownload(d);
        else if (running) running.abort.abort();
      } else {
        forwardDown(d);
      }
    } else {
      // SOL_CAPTURED / SOL_PROGRESS / SOL_DONE / SOL_ERROR from a child frame.
      reportUp(d);
    }
  });

  // ── DASH pipeline (adapted from content.js) ──────────────────────────────

  function svcMsFetchInit(extra) {
    var init = {};
    for (var k in (extra || {})) init[k] = extra[k];
    if (capture && capture.spopactoken) {
      init.headers = { 'x-spopactoken': capture.spopactoken };
    }
    return init;
  }

  function parseDashManifest(xmlText, manifestUrl) {
    var doc = new DOMParser().parseFromString(xmlText, 'application/xml');
    if (doc.querySelector('parsererror')) throw new Error('Failed to parse DASH manifest XML');

    var baseUrlEl = doc.querySelector('BaseURL');
    var manifestDerivedBase = manifestUrl.split('?')[0].replace(/\/[^/]*$/, '/');
    var baseUrl = (baseUrlEl && baseUrlEl.textContent.trim()) || manifestDerivedBase;

    function toAbsolute(url) {
      if (!url) return '';
      if (/^https:\/\//.test(url)) return url;
      if (/^[a-z][a-z0-9+\-.]*:/i.test(url)) throw new Error('Unsafe URL scheme in manifest: ' + url);
      return new URL(url, baseUrl).href;
    }
    function expandTemplate(tpl, repId, bandwidth, number, time) {
      return tpl
        .replace(/\$RepresentationID\$/g, repId)
        .replace(/\$Bandwidth\$/g, bandwidth)
        .replace(/\$Number%0(\d+)d\$/g, function (_, w) { return String(number).padStart(parseInt(w, 10), '0'); })
        .replace(/\$Number\$/g, String(number))
        .replace(/\$Time\$/g, String(time));
    }

    var adaptationSets = Array.prototype.slice.call(doc.querySelectorAll('AdaptationSet'));
    var isMuxed = adaptationSets.length === 1;
    var tracks = [];

    for (var i = 0; i < adaptationSets.length; i++) {
      var as = adaptationSets[i];
      var type = as.getAttribute('contentType') || '';
      if (!type) {
        var mime = as.getAttribute('mimeType') || '';
        type = mime.indexOf('video') === 0 ? 'video' : mime.indexOf('audio') === 0 ? 'audio' : '';
      }
      if (isMuxed) type = 'muxed';

      var reps = Array.prototype.slice.call(as.querySelectorAll('Representation'))
        .sort(function (a, b) { return parseInt(b.getAttribute('bandwidth') || '0', 10) - parseInt(a.getAttribute('bandwidth') || '0', 10); });
      var rep = reps[0];
      if (!rep) continue;

      var repId = rep.getAttribute('id') || '';
      var bandwidth = rep.getAttribute('bandwidth') || '';

      var segTpl = rep.querySelector('SegmentTemplate') || as.querySelector('SegmentTemplate');
      if (!segTpl) continue;

      var startNumber = parseInt(segTpl.getAttribute('startNumber') || '1', 10);
      var initTpl = segTpl.getAttribute('initialization') || '';
      var mediaTpl = segTpl.getAttribute('media') || '';
      var initUrl = toAbsolute(expandTemplate(initTpl, repId, bandwidth, startNumber, 0));
      var segments = [];

      var timeline = segTpl.querySelector('SegmentTimeline');
      if (timeline) {
        var t = 0, segNum = startNumber;
        var ss = timeline.querySelectorAll('S');
        for (var si = 0; si < ss.length; si++) {
          var s = ss[si];
          var sT = s.getAttribute('t');
          if (sT !== null) t = parseInt(sT, 10);
          var d = parseInt(s.getAttribute('d') || '0', 10);
          var r = parseInt(s.getAttribute('r') || '0', 10);
          for (var ri = 0; ri <= r; ri++) {
            segments.push(toAbsolute(expandTemplate(mediaTpl, repId, bandwidth, segNum, t)));
            t += d;
            segNum++;
          }
        }
      } else {
        var duration = parseInt(segTpl.getAttribute('duration') || '0', 10);
        var timescale = parseInt(segTpl.getAttribute('timescale') || '1', 10);
        var period = as.closest ? as.closest('Period') : null;
        var periodDur = 0;
        if (period) {
          var m = (period.getAttribute('duration') || '').match(/PT(?:(\d+)H)?(?:(\d+)M)?(?:([\d.]+)S)?/);
          periodDur = m ? parseInt(m[1] || '0') * 3600 + parseInt(m[2] || '0') * 60 + parseFloat(m[3] || '0') : 0;
        }
        if (duration > 0 && periodDur > 0) {
          var count = Math.ceil(periodDur / (duration / timescale));
          for (var pi = 0; pi < count; pi++) {
            segments.push(toAbsolute(expandTemplate(mediaTpl, repId, bandwidth, startNumber + pi, pi * duration)));
          }
        }
      }

      var encryption = null;
      var cps = as.querySelectorAll('ContentProtection');
      for (var ci = 0; ci < cps.length; ci++) {
        if (cps[ci].getAttribute('schemeIdUri') === 'urn:mpeg:dash:sea:2012') {
          var segEnc = cps[ci].querySelector('SegmentEncryption');
          var scheme = segEnc ? segEnc.getAttribute('schemeIdUri') : '';
          var periodEl = cps[ci].querySelector('CryptoPeriod');
          var keyUri = periodEl ? periodEl.getAttribute('keyUriTemplate') : null;
          var ivAttr = periodEl ? (periodEl.getAttribute('IV') || '') : '';
          if (/aes128-cbc/i.test(scheme) && keyUri && ivAttr) {
            encryption = { scheme: 'aes-128-cbc', keyUri: keyUri, iv: hexToBytes(ivAttr.replace(/^0x/i, '')) };
          }
          break;
        }
      }

      tracks.push({ type: type, initUrl: initUrl, segments: segments, encryption: encryption });
    }

    return tracks;
  }

  function hexToBytes(hex) {
    var out = new Uint8Array(hex.length / 2);
    for (var i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
    return out;
  }

  function abortableSleep(ms, signal) {
    return new Promise(function (resolve, reject) {
      if (signal && signal.aborted) { reject(aborted()); return; }
      var t = setTimeout(function () {
        if (signal) signal.removeEventListener('abort', onAbort);
        resolve();
      }, ms);
      function onAbort() { clearTimeout(t); reject(aborted()); }
      if (signal) signal.addEventListener('abort', onAbort, { once: true });
    });
  }
  function aborted() { return Object.assign(new Error('Cancelled'), { name: 'AbortError' }); }

  function fetchWithRetry(url, init, signal, onThrottle, maxAttempts) {
    maxAttempts = maxAttempts || 6;
    var attempt = 0;
    return new Promise(function (resolve, reject) {
      function loop() {
        attempt++;
        if (signal && signal.aborted) { reject(aborted()); return; }
        fetch(url, init).then(function (resp) {
          if ((resp.status === 429 || resp.status === 503) && attempt < maxAttempts) {
            var headerSecs = parseInt(resp.headers.get('Retry-After'), 10);
            var delayMs = Number.isFinite(headerSecs) && headerSecs > 0
              ? Math.min(headerSecs * 1000, 30000)
              : Math.min(1000 * Math.pow(2, attempt - 1), 30000);
            if (onThrottle) onThrottle({ attempt: attempt, delayMs: delayMs, status: resp.status });
            abortableSleep(delayMs, signal).then(loop, reject);
            return;
          }
          resolve(resp);
        }, function (e) {
          if (e.name === 'AbortError') { reject(e); return; }
          if (attempt >= maxAttempts) { reject(e); return; }
          var delayMs = Math.min(1000 * Math.pow(2, attempt - 1), 30000);
          if (onThrottle) onThrottle({ attempt: attempt, delayMs: delayMs, status: 0 });
          abortableSleep(delayMs, signal).then(loop, reject);
        });
      }
      loop();
    });
  }

  function downloadDashSegments(tracks, onProgress, signal, concurrency) {
    // User setting from the app (clamped): segments in flight across all tracks.
    concurrency = Math.min(16, Math.max(1, parseInt(concurrency, 10) || 4));

    var totalSegs = tracks.reduce(function (s, t) { return s + (t.initUrl ? 1 : 0) + t.segments.length; }, 0);
    var done = 0;

    function reportProgress(text) { onProgress(done, totalSegs, text); }
    function noteThrottle(info) {
      reportProgress('HTTP ' + (info.status || 'network') + ' — backing off ' + Math.round(info.delayMs / 1000) + 's (attempt ' + info.attempt + ')...');
    }

    var trackStates = Promise.all(tracks.map(function (track) {
      var label = tracks.length > 1 ? ' (' + track.type + ' track)' : '';

      var cryptoKey = null;
      var keyPromise = Promise.resolve();
      if (track.encryption) {
        reportProgress('Fetching encryption key' + label + '...');
        var init = segFetchInit(track.encryption.keyUri, signal);
        keyPromise = fetchWithRetry(track.encryption.keyUri, init, signal, noteThrottle).then(function (keyResp) {
          if (!keyResp.ok) throw new Error('Encryption key fetch failed: HTTP ' + keyResp.status);
          return keyResp.arrayBuffer();
        }).then(function (keyBuf) {
          return crypto.subtle.importKey('raw', keyBuf, { name: 'AES-CBC' }, false, ['decrypt']);
        });
      }

      return keyPromise.then(function (key) {
        cryptoKey = key;
        function decryptIfNeeded(buf) {
          if (!cryptoKey) return buf;
          return crypto.subtle.decrypt({ name: 'AES-CBC', iv: track.encryption.iv }, cryptoKey, buf);
        }

        var orderedBufs = new Array((track.initUrl ? 1 : 0) + track.segments.length);
        var segStart = 0;
        var headPromise = Promise.resolve();
        if (track.initUrl) {
          reportProgress('Fetching init segment' + label + '...');
          headPromise = fetchWithRetry(track.initUrl, segFetchInit(track.initUrl, signal), signal, noteThrottle).then(function (r) {
            if (!r.ok) throw new Error('Init segment failed: HTTP ' + r.status + ' (' + hostOf(track.initUrl) + ')');
            return r.arrayBuffer();
          }).then(function (buf) {
            return decryptIfNeeded(buf);
          }).then(function (buf) {
            orderedBufs[0] = buf;
            done++;
            segStart = 1;
          });
        }
        return headPromise.then(function () {
          return { track: track, label: label, orderedBufs: orderedBufs, segStart: segStart, decryptIfNeeded: decryptIfNeeded };
        });
      });
    }));

    return trackStates.then(function (states) {
      var queue = [];
      states.forEach(function (st) {
        for (var i = 0; i < st.track.segments.length; i++) queue.push({ st: st, i: i });
      });
      reportProgress('Downloading ' + queue.length + ' segments (' + concurrency + ' parallel)...');

      return new Promise(function (resolve, reject) {
        if (queue.length === 0) { resolve(states); return; }
        var qIdx = 0, inFlight = 0, failed = false;

        function launch() {
          while (!failed && inFlight < concurrency && qIdx < queue.length) {
            if (signal && signal.aborted) { failed = true; reject(aborted()); return; }
            var job = queue[qIdx++];
            inFlight++;
            fetchWithRetry(job.st.track.segments[job.i], segFetchInit(job.st.track.segments[job.i], signal), signal, noteThrottle)
              .then(function (r) {
                if (!r.ok) throw new Error('Segment failed: HTTP ' + r.status + ' (' + hostOf(job.st.track.segments[job.i]) + ')');
                return r.arrayBuffer();
              })
              .then(job.st.decryptIfNeeded)
              .then(function (buf) {
                // Write FIRST: a transient `failed` flag (from a sibling job's
                // error) must never leave this slot empty while the queue
                // still resolves.
                job.st.orderedBufs[job.st.segStart + job.i] = buf;
                if (failed) return;
                done++;
                reportProgress('Downloading segments... (' + done + '/' + totalSegs + ')');
                inFlight--;
                if (inFlight === 0 && qIdx >= queue.length) resolve(states);
                else launch();
              }, function (err) {
                if (failed) return;
                failed = true;
                reject(err);
              });
          }
        }
        launch();
      });
    }).then(function (states) {
      return states;
    });
  }

  // ── Mux worker (verbatim logic from mux-worker.js, spawned from a blob) ──
  var MUX_SOURCE = `function mux(videoUint8, audioUint8, onProgress) {
  function readU32(b, off) {
    return ((b[off] << 24) | (b[off+1] << 16) | (b[off+2] << 8) | b[off+3]) >>> 0;
  }
  function writeU32(b, off, val) {
    b[off] = (val >>> 24) & 0xFF; b[off+1] = (val >>> 16) & 0xFF;
    b[off+2] = (val >>> 8) & 0xFF; b[off+3] = val & 0xFF;
  }
  function btype(b, off) {
    return String.fromCharCode(b[off], b[off+1], b[off+2], b[off+3]);
  }
  function cat(...arrays) {
    const out = new Uint8Array(arrays.reduce((s, a) => s + a.byteLength, 0));
    let o = 0; for (const a of arrays) { out.set(a, o); o += a.byteLength; }
    return out;
  }
  function findBox(b, type, startOff = 0, maxOff = b.length) {
    let pos = startOff;
    while (pos + 8 <= maxOff) {
      const size = readU32(b, pos);
      if (size < 8) break;
      if (btype(b, pos + 4) === type) return { offset: pos, size };
      pos += size;
    }
    return null;
  }
  onProgress(0, 1, 'Muxing tracks...');
  if (!audioUint8) {
    // Single-track input: strip anything before the ftyp and keep the
    // fragmented structure — [ftyp, moov, moof, mdat, ...] is a valid fMP4.
    const ftypBox = findBox(videoUint8, 'ftyp');
    if (!ftypBox) return videoUint8;
    return videoUint8.slice(ftypBox.offset);
  }
  const vMoovBox = findBox(videoUint8, 'moov');
  const aMoovBox = findBox(audioUint8, 'moov');
  if (!vMoovBox) throw new Error('No moov found in video buffer');
  if (!aMoovBox) throw new Error('No moov found in audio buffer');
  const vMoov = videoUint8.slice(vMoovBox.offset, vMoovBox.offset + vMoovBox.size);
  const aMoov = audioUint8.slice(aMoovBox.offset, aMoovBox.offset + aMoovBox.size);
  const aTrakBox = findBox(aMoov, 'trak', 8);
  if (!aTrakBox) throw new Error('No trak in audio moov');
  const aTrak = new Uint8Array(aMoov.slice(aTrakBox.offset, aTrakBox.offset + aTrakBox.size));
  const aTkhdBox = findBox(aTrak, 'tkhd', 8);
  if (aTkhdBox) {
    const v = aTrak[aTkhdBox.offset + 8];
    writeU32(aTrak, aTkhdBox.offset + (v === 1 ? 28 : 20), 2);
  }
  let aTrex;
  const aMvexBox = findBox(aMoov, 'mvex', 8);
  if (aMvexBox) {
    const aTrexBox = findBox(aMoov, 'trex', aMvexBox.offset + 8);
    if (aTrexBox) {
      aTrex = new Uint8Array(aMoov.slice(aTrexBox.offset, aTrexBox.offset + aTrexBox.size));
      writeU32(aTrex, 12, 2);
    }
  }
  if (!aTrex) {
    aTrex = new Uint8Array([
      0x00,0x00,0x00,0x20, 0x74,0x72,0x65,0x78,
      0x00,0x00,0x00,0x00,
      0x00,0x00,0x00,0x02,
      0x00,0x00,0x00,0x01,
      0x00,0x00,0x00,0x00,
      0x00,0x00,0x00,0x00,
      0x00,0x00,0x00,0x00,
    ]);
  }
  const workMoov = new Uint8Array(vMoov);
  const vMvhdBox = findBox(workMoov, 'mvhd', 8);
  if (vMvhdBox) {
    const v = workMoov[vMvhdBox.offset + 8];
    writeU32(workMoov, vMvhdBox.offset + (v === 1 ? 116 : 104), 3);
  }
  const vMvexBox = findBox(workMoov, 'mvex', 8);
  let combinedMoov;
  if (vMvexBox) {
    const oldMvex = workMoov.slice(vMvexBox.offset, vMvexBox.offset + vMvexBox.size);
    const newMvex = cat(oldMvex, aTrex);
    writeU32(newMvex, 0, newMvex.length);
    const beforeMvex  = workMoov.slice(8, vMvexBox.offset);
    const afterMvex   = workMoov.slice(vMvexBox.offset + vMvexBox.size);
    const moovContent = cat(beforeMvex, aTrak, newMvex, afterMvex);
    combinedMoov = new Uint8Array(8 + moovContent.length);
    writeU32(combinedMoov, 0, combinedMoov.length);
    combinedMoov.set([0x6D,0x6F,0x6F,0x76], 4);
    combinedMoov.set(moovContent, 8);
  } else {
    const vTrex = new Uint8Array([
      0x00,0x00,0x00,0x20, 0x74,0x72,0x65,0x78,
      0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x01,
      0x00,0x00,0x00,0x01, 0x00,0x00,0x00,0x00,
      0x00,0x00,0x00,0x00, 0x00,0x00,0x00,0x00,
    ]);
    const mvexContent = cat(vTrex, aTrex);
    const mvex = new Uint8Array(8 + mvexContent.length);
    writeU32(mvex, 0, mvex.length);
    mvex.set([0x6D,0x76,0x65,0x78], 4);
    mvex.set(mvexContent, 8);
    const moovContent = cat(workMoov.slice(8), aTrak, mvex);
    combinedMoov = new Uint8Array(8 + moovContent.length);
    writeU32(combinedMoov, 0, combinedMoov.length);
    combinedMoov.set([0x6D,0x6F,0x6F,0x76], 4);
    combinedMoov.set(moovContent, 8);
  }
  function collectFragments(bytes) {
    const frags = [];
    let pos = 0;
    while (pos + 8 <= bytes.length) {
      const size = readU32(bytes, pos);
      if (size < 8) break;
      if (btype(bytes, pos + 4) === 'moof') {
        let trafData = null;
        let mp = pos + 8;
        while (mp + 8 <= pos + size) {
          const csz = readU32(bytes, mp);
          if (csz < 8) break;
          if (btype(bytes, mp + 4) === 'traf') {
            trafData = bytes.slice(mp, mp + csz);
            break;
          }
          mp += csz;
        }
        const nextPos = pos + size;
        let mdatPayload = null;
        if (nextPos + 8 <= bytes.length && btype(bytes, nextPos + 4) === 'mdat') {
          const mdatSize = readU32(bytes, nextPos);
          mdatPayload = bytes.slice(nextPos + 8, nextPos + mdatSize);
        }
        if (trafData && mdatPayload) frags.push({ traf: trafData, mdatPayload });
      }
      pos += size;
    }
    return frags;
  }
  function parseTrafSamples(trafBytes) {
    let defDur = 0, defSize = 0, defFlags = 0;
    const samples = [];
    let pos = 8;
    while (pos + 8 <= trafBytes.length) {
      const sz = readU32(trafBytes, pos);
      if (sz < 8) break;
      const t = btype(trafBytes, pos + 4);
      if (t === 'tfhd') {
        const fl = ((trafBytes[pos+9]<<16)|(trafBytes[pos+10]<<8)|trafBytes[pos+11])>>>0;
        let o = pos + 16;
        if (fl & 1) o += 8;
        if (fl & 2) o += 4;
        if (fl & 8) { defDur = readU32(trafBytes, o); o += 4; }
        if (fl & 0x10) { defSize = readU32(trafBytes, o); o += 4; }
        if (fl & 0x20) { defFlags = readU32(trafBytes, o); o += 4; }
      }
      if (t === 'trun') {
        const fl = ((trafBytes[pos+9]<<16)|(trafBytes[pos+10]<<8)|trafBytes[pos+11])>>>0;
        const cnt = readU32(trafBytes, pos + 12);
        let o = pos + 16;
        if (fl & 1) o += 4;
        let firstFlags = defFlags;
        if (fl & 4) { firstFlags = readU32(trafBytes, o); o += 4; }
        for (let i = 0; i < cnt; i++) {
          let dur = defDur, size = defSize, flags = (i === 0) ? firstFlags : defFlags, cts = 0;
          if (fl & 0x100) { dur = readU32(trafBytes, o); o += 4; }
          if (fl & 0x200) { size = readU32(trafBytes, o); o += 4; }
          if (fl & 0x400) { flags = readU32(trafBytes, o); o += 4; }
          if (fl & 0x800) { cts = readU32(trafBytes, o); o += 4; }
          samples.push({ duration: dur, size, flags, ctsOffset: cts });
        }
      }
      pos += sz;
    }
    return samples;
  }
  const vFrags = collectFragments(videoUint8);
  const aFrags = collectFragments(audioUint8);
  const vSamples = vFrags.flatMap(f => parseTrafSamples(f.traf));
  const aSamples = aFrags.flatMap(f => parseTrafSamples(f.traf));
  const vData = cat(...vFrags.map(f => f.mdatPayload));
  const aData = cat(...aFrags.map(f => f.mdatPayload));
  onProgress(0, 1, 'Building MP4...');
  function makeBox(type, ...contents) {
    const totalContent = contents.reduce((s, c) => s + c.byteLength, 0);
    const box = new Uint8Array(8 + totalContent);
    writeU32(box, 0, box.length);
    for (let i = 0; i < 4; i++) box[4 + i] = type.charCodeAt(i);
    let off = 8;
    for (const c of contents) { box.set(c, off); off += c.byteLength; }
    return box;
  }
  function makeFullBox(type, version, flags, content) {
    const vf = new Uint8Array(4);
    vf[0] = version;
    vf[1] = (flags >> 16) & 0xFF; vf[2] = (flags >> 8) & 0xFF; vf[3] = flags & 0xFF;
    return makeBox(type, vf, content);
  }
  function buildStts(samples) {
    const runs = [];
    for (const s of samples) {
      if (runs.length > 0 && runs[runs.length - 1].dur === s.duration) {
        runs[runs.length - 1].count++;
      } else {
        runs.push({ count: 1, dur: s.duration });
      }
    }
    const data = new Uint8Array(4 + 4 + runs.length * 8);
    writeU32(data, 4, runs.length);
    for (let i = 0; i < runs.length; i++) {
      writeU32(data, 8 + i * 8, runs[i].count);
      writeU32(data, 12 + i * 8, runs[i].dur);
    }
    return makeBox('stts', data);
  }
  function buildStsz(samples) {
    const allSame = samples.length > 0 && samples.every(s => s.size === samples[0].size);
    const data = new Uint8Array(4 + 4 + 4 + (allSame ? 0 : samples.length * 4));
    writeU32(data, 4, allSame ? samples[0].size : 0);
    writeU32(data, 8, samples.length);
    if (!allSame) {
      for (let i = 0; i < samples.length; i++) {
        writeU32(data, 12 + i * 4, samples[i].size);
      }
    }
    return makeBox('stsz', data);
  }
  function buildStsc() {
    const data = new Uint8Array(4 + 4 + 12);
    writeU32(data, 4, 1);
    writeU32(data, 8, 1);
    writeU32(data, 12, 0);
    writeU32(data, 16, 1);
    return makeBox('stsc', data);
  }
  function buildStco() {
    const data = new Uint8Array(4 + 4 + 4);
    writeU32(data, 4, 1);
    writeU32(data, 8, 0);
    return makeBox('stco', data);
  }
  function buildStss(samples) {
    const syncIndices = [];
    for (let i = 0; i < samples.length; i++) {
      if (!(samples[i].flags & 0x10000)) syncIndices.push(i + 1);
    }
    const data = new Uint8Array(4 + 4 + syncIndices.length * 4);
    writeU32(data, 4, syncIndices.length);
    for (let i = 0; i < syncIndices.length; i++) {
      writeU32(data, 8 + i * 4, syncIndices[i]);
    }
    return makeBox('stss', data);
  }
  function buildCtts(samples) {
    if (samples.every(s => s.ctsOffset === 0)) return null;
    const runs = [];
    for (const s of samples) {
      if (runs.length > 0 && runs[runs.length - 1].offset === s.ctsOffset) {
        runs[runs.length - 1].count++;
      } else {
        runs.push({ count: 1, offset: s.ctsOffset });
      }
    }
    const data = new Uint8Array(4 + 4 + runs.length * 8);
    writeU32(data, 4, runs.length);
    for (let i = 0; i < runs.length; i++) {
      writeU32(data, 8 + i * 8, runs[i].count);
      writeU32(data, 12 + i * 8, runs[i].offset);
    }
    return makeBox('ctts', data);
  }
  function extractBox(parent, type, startOff, maxOff) {
    const box = findBox(parent, type, startOff || 0, maxOff || parent.length);
    return box ? parent.slice(box.offset, box.offset + box.size) : null;
  }
  const existingMvhd = extractBox(combinedMoov, 'mvhd', 8);
  const traks = [];
  let tp = 8;
  while (tp + 8 <= combinedMoov.length) {
    const sz = readU32(combinedMoov, tp);
    if (sz < 8) break;
    if (btype(combinedMoov, tp + 4) === 'trak') {
      traks.push(combinedMoov.slice(tp, tp + sz));
    }
    tp += sz;
  }
  function extractFromTrak(trak) {
    const tkhd = extractBox(trak, 'tkhd', 8);
    const mdiaBox = findBox(trak, 'mdia', 8);
    const mdia = mdiaBox ? trak.slice(mdiaBox.offset, mdiaBox.offset + mdiaBox.size) : null;
    let mdhd = null, hdlr = null, stsd = null, isVideo = false;
    if (mdia) {
      mdhd = extractBox(mdia, 'mdhd', 8);
      hdlr = extractBox(mdia, 'hdlr', 8);
      if (hdlr) {
        isVideo = btype(hdlr, 16) === 'vide';
      }
      const minfBox = findBox(mdia, 'minf', 8);
      if (minfBox) {
        const minf = mdia.slice(minfBox.offset, minfBox.offset + minfBox.size);
        const stblBox = findBox(minf, 'stbl', 8);
        if (stblBox) {
          const stbl = minf.slice(stblBox.offset, stblBox.offset + stblBox.size);
          stsd = extractBox(stbl, 'stsd', 8);
        }
        const vmhd = extractBox(minf, 'vmhd', 8);
        const smhd = extractBox(minf, 'smhd', 8);
        return { tkhd, mdhd, hdlr, stsd, isVideo, xmhd: vmhd || smhd };
      }
    }
    return { tkhd, mdhd, hdlr, stsd, isVideo, xmhd: null };
  }
  const vTrakInfo = extractFromTrak(traks[0]);
  const aTrakInfo = extractFromTrak(traks[1]);
  function buildTrak(info, samples, sampleCount) {
    const stts = buildStts(samples);
    const stsz = buildStsz(samples);
    const stss = info.isVideo ? buildStss(samples) : null;
    const ctts = buildCtts(samples);
    const stsc = buildStsc();
    writeU32(stsc, 20, sampleCount);
    const stco = buildStco();
    const urlBox = makeFullBox('url ', 0, 1, new Uint8Array(0));
    const drefData = new Uint8Array(4 + 4);
    writeU32(drefData, 4, 1);
    const dref = makeBox('dref', drefData, urlBox);
    const dinf = makeBox('dinf', dref);
    const stblParts = [info.stsd, stts, stsc, stsz, stco];
    if (stss) stblParts.push(stss);
    if (ctts) stblParts.push(ctts);
    const stbl = makeBox('stbl', ...stblParts);
    const minf = makeBox('minf', info.xmhd, dinf, stbl);
    const mdia = makeBox('mdia', info.mdhd, info.hdlr, minf);
    const trak = makeBox('trak', info.tkhd, mdia);
    return trak;
  }
  const newVTrak = buildTrak(vTrakInfo, vSamples, vSamples.length);
  const newATrak = buildTrak(aTrakInfo, aSamples, aSamples.length);
  function getMdhdTimescale(mdhd) {
    const v = mdhd[8];
    return readU32(mdhd, v === 1 ? 28 : 20);
  }
  const vTimescale = getMdhdTimescale(vTrakInfo.mdhd);
  const aTimescale = getMdhdTimescale(aTrakInfo.mdhd);
  const vTotalDur = vSamples.reduce((s, x) => s + x.duration, 0);
  const aTotalDur = aSamples.reduce((s, x) => s + x.duration, 0);
  function patchMdhdDuration(trak, duration) {
    const mdiaBox = findBox(trak, 'mdia', 8);
    if (!mdiaBox) return;
    const mdhdBox = findBox(trak, 'mdhd', mdiaBox.offset + 8, mdiaBox.offset + mdiaBox.size);
    if (!mdhdBox) return;
    const v = trak[mdhdBox.offset + 8];
    writeU32(trak, mdhdBox.offset + (v === 1 ? 32 : 24), duration);
  }
  function patchTkhdDuration(trak, movieDuration) {
    const tkhdBox = findBox(trak, 'tkhd', 8);
    if (!tkhdBox) return;
    const v = trak[tkhdBox.offset + 8];
    writeU32(trak, tkhdBox.offset + (v === 1 ? 36 : 28), movieDuration);
  }
  patchMdhdDuration(newVTrak, vTotalDur);
  patchMdhdDuration(newATrak, aTotalDur);
  const mvhdV = existingMvhd[8];
  const movieTimescale = readU32(existingMvhd, mvhdV === 1 ? 28 : 20);
  const vMovieDur = Math.round(vTotalDur * movieTimescale / vTimescale);
  const aMovieDur = Math.round(aTotalDur * movieTimescale / aTimescale);
  const maxMovieDur = Math.max(vMovieDur, aMovieDur);
  writeU32(existingMvhd, mvhdV === 1 ? 32 : 24, maxMovieDur);
  patchTkhdDuration(newVTrak, vMovieDur);
  patchTkhdDuration(newATrak, aMovieDur);
  const newMoov = makeBox('moov', existingMvhd, newVTrak, newATrak);
  const vFtypBox = findBox(videoUint8, 'ftyp');
  const ftyp = vFtypBox ? videoUint8.slice(vFtypBox.offset, vFtypBox.offset + vFtypBox.size) : new Uint8Array(0);
  const mdatPayload = cat(vData, aData);
  const mdatBox = new Uint8Array(8 + mdatPayload.length);
  writeU32(mdatBox, 0, mdatBox.length);
  mdatBox[4]=0x6D; mdatBox[5]=0x64; mdatBox[6]=0x61; mdatBox[7]=0x74;
  mdatBox.set(mdatPayload, 8);
  const videoDataOffset = ftyp.length + newMoov.length + 8;
  const audioDataOffset = videoDataOffset + vData.length;
  function patchStcoInMoov(moov, trakIndex, offset) {
    let trakCount = 0;
    let p = 8;
    while (p + 8 <= moov.length) {
      const sz = readU32(moov, p);
      if (sz < 8) break;
      if (btype(moov, p + 4) === 'trak') {
        if (trakCount === trakIndex) {
          const stcoBox = (function findDeep(buf, type, start, end) {
            let pos = start;
            while (pos + 8 <= end) {
              const s = readU32(buf, pos);
              if (s < 8) break;
              if (btype(buf, pos + 4) === type) return pos;
              const inner = findDeep(buf, type, pos + 8, pos + s);
              if (inner !== -1) return inner;
              pos += s;
            }
            return -1;
          })(moov, 'stco', p + 8, p + sz);
          if (stcoBox !== -1) {
            writeU32(moov, stcoBox + 16, offset);
          }
          return;
        }
        trakCount++;
      }
      p += sz;
    }
  }
  patchStcoInMoov(newMoov, 0, videoDataOffset);
  patchStcoInMoov(newMoov, 1, audioDataOffset);
  return cat(ftyp, newMoov, mdatBox);
}`;

  var MUX_WORKER_SRC = [
    'self.addEventListener(\'message\', async (event) => {',
    '  const { video, audio } = event.data || {};',
    '  function reportProgress(done, total, text) { self.postMessage({ progress: { done, total, text } }); }',
    '  function concatChunks(chunks) {',
    '    const total = chunks.reduce((s, b) => { if (b == null) throw new Error("missing buffer in " + (video && audio ? "track pair" : "single track") + " (length " + chunks.length + ")"); return s + b.byteLength; }, 0);',
    '    const out = new Uint8Array(total);',
    '    let off = 0;',
    '    for (const b of chunks) { out.set(b instanceof Uint8Array ? b : new Uint8Array(b), off); off += b.byteLength; }',
    '    return out;',
    '  }',
    '  try {',
    '    let result;',
    '    if (video && audio) result = mux(concatChunks(video), concatChunks(audio), reportProgress);',
    '    else { const only = concatChunks(video || audio); result = only; }',
    '    self.postMessage({ result }, [result.buffer]);',
    '  } catch (err) {',
    '    self.postMessage({ error: err && err.message ? err.message : String(err) });',
    '  }',
    '});',
    MUX_SOURCE
  ].join('\n');

  function muxTracks(videoChunks, audioChunks, onProgress) {
    return new Promise(function (resolve, reject) {
      var worker;
      try {
        worker = new Worker(URL.createObjectURL(new Blob([MUX_WORKER_SRC], { type: 'application/javascript' })));
      } catch (e) { reject(e); return; }
      worker.onmessage = function (event) {
        var msg = event.data;
        if (msg.progress) onProgress(msg.progress.done, msg.progress.total, msg.progress.text);
        else if (msg.error) { worker.terminate(); reject(new Error(msg.error)); }
        else if (msg.result) { worker.terminate(); resolve(msg.result); }
      };
      worker.onerror = function (e) { worker.terminate(); reject(new Error(e.message || 'mux-worker crashed')); };
      // Transfer the buffers so a 1 GB recording isn't copied to the worker.
      var transferables = videoChunks.concat(audioChunks || []).filter(function (b) { return b instanceof ArrayBuffer; });
      worker.postMessage({ video: videoChunks, audio: audioChunks }, transferables);
    });
  }

  function singleTrackMux(chunks) {
    return new Promise(function (resolve, reject) {
      var worker;
      try {
        worker = new Worker(URL.createObjectURL(new Blob([MUX_WORKER_SRC], { type: 'application/javascript' })));
      } catch (e) { reject(e); return; }
      worker.onmessage = function (event) {
        var msg = event.data;
        if (msg.progress) { /* single track: no mux, just concat */ }
        else if (msg.error) { worker.terminate(); reject(new Error(msg.error)); }
        else if (msg.result) { worker.terminate(); resolve(msg.result); }
      };
      worker.onerror = function (e) { worker.terminate(); reject(new Error(e.message || 'mux-worker crashed')); };
      var transferables = chunks.filter(function (b) { return b instanceof ArrayBuffer; });
      worker.postMessage({ video: chunks }, transferables);
    });
  }

  function runDownload(cmd) {
    if (running) return;
    var abort = new AbortController();
    running = { captureId: cmd.captureId, abort: abort };
    var cid = cmd.captureId;

    function progress(done, total, text) { reportUp({ type: 'SOL_PROGRESS', captureId: cid, done: done, total: total, text: text }); }

    (async function () {
      progress(0, 1, 'Fetching manifest...');
      var resp = await fetchWithRetry(capture.manifestUrl, svcMsFetchInit({ signal: abort.signal }), abort.signal, null);
      if (!resp.ok) throw new Error('Manifest fetch failed: HTTP ' + resp.status);
      var xmlText = await resp.text();

      progress(0, 1, 'Parsing manifest...');
      var allTracks = parseDashManifest(xmlText, capture.manifestUrl);
      if (!allTracks.length) throw new Error('No tracks found in manifest');

      var HARD_DRM_SCHEMES = [
        'edef8ba9-79d6-4ace-a3c8-27dcd51d21ed', // Widevine
        '9a04f079-9840-4286-ab92-e65be0885f95', // PlayReady
        '94ce86fb-07ff-4f43-adb8-93d2fa968ca2'  // FairPlay
      ];
      var cpSchemes = [];
      xmlText.replace(/<ContentProtection\b[^>]*schemeIdUri="([^"]+)"/gi, function (_, s) { cpSchemes.push(s.toLowerCase()); return _; });
      var hasHardDrm = cpSchemes.some(function (s) {
        return HARD_DRM_SCHEMES.some(function (u) { return s.indexOf(u) !== -1; });
      });
      if (hasHardDrm) {
        var drmErr = new Error('DRM protected recording');
        drmErr.isDrm = true;
        throw drmErr;
      }

      var videoTrack = allTracks.find(function (t) { return t.type === 'video' || t.type === 'muxed'; });
      var audioTrack = allTracks.find(function (t) { return t.type === 'audio'; });

      var tracksToDownload;
      var isSeparate = false;
      if (!audioTrack || allTracks.length === 1) {
        tracksToDownload = [videoTrack || allTracks[0]];
      } else {
        tracksToDownload = [videoTrack, audioTrack].filter(Boolean);
        isSeparate = tracksToDownload.length > 1;
      }

      var states = await downloadDashSegments(tracksToDownload, progress, abort.signal, cmd.concurrency);
      var trackData = states.map(function (s) { return s.orderedBufs; });

      // Self-heal: any slot that somehow arrived empty is re-fetched directly
      // from its URL (with the right auth headers), decrypted, and refilled.
      // Two bounded rounds; then a strict check so we never hand the muxer a
      // sparse array (the source of the opaque "reading 'byteLength'" error).
      for (var healRound = 0; healRound < 2; healRound++) {
        var missing = [];
        states.forEach(function (st, ti) {
          for (var k = 0; k < st.orderedBufs.length; k++) {
            if (st.orderedBufs[k] == null) missing.push({ st: st, ti: ti, k: k });
          }
        });
        if (!missing.length) break;
        console.error('[ShareOneList] healing ' + missing.length + ' missing segment buffer(s): ' +
          missing.map(function (m) { return 'track' + m.ti + '#' + m.k; }).join(','));
        progress('Re-fetching ' + missing.length + ' missing segment(s)...');
        await Promise.all(missing.map(function (m) {
          var st = m.st;
          var url = m.k === 0 && st.track.initUrl ? st.track.initUrl : st.track.segments[m.k - st.segStart];
          return fetchWithRetry(url, segFetchInit(url), abort.signal, null).then(function (r) {
            if (!r.ok) throw new Error('Heal fetch failed: HTTP ' + r.status + ' (' + hostOf(url) + ')');
            return r.arrayBuffer();
          }).then(st.decryptIfNeeded).then(function (buf) {
            st.orderedBufs[m.k] = buf;
          });
        }));
      }

      trackData.forEach(function (chunks, ti) {
        for (var k = 0; k < chunks.length; k++) {
          if (chunks[k] == null) {
            throw new Error('Track ' + ti + ' still missing buffer ' + k + ' after heal rounds');
          }
        }
      });

      var result;
      if (isSeparate) {
        result = await muxTracks(trackData[0], trackData[1], progress);
      } else {
        result = await singleTrackMux(trackData[0]);
      }
      progress(1, 1, 'Saving...');

      var uploadUrl = 'http://127.0.0.1:' + cmd.port + '/upload?token=' + encodeURIComponent(cmd.uploadToken);
      var blob = new Blob([result], { type: 'video/mp4' });
      var uploadResp = await fetch(uploadUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/octet-stream' },
        body: blob
      });
      if (!uploadResp.ok) throw new Error('Save failed: HTTP ' + uploadResp.status);
      reportUp({ type: 'SOL_DONE', captureId: cid });
    })().catch(function (err) {
      console.error('[ShareOneList] stream pipeline failed:', err);
      reportUp({
        type: 'SOL_ERROR',
        captureId: cid,
        message: (err && err.message) || String(err),
        isDrm: !!(err && err.isDrm)
      });
    }).finally(function () {
      running = null;
    });
  }
})();
