(function () {
  var data = JSON.parse(document.getElementById("seer-graph").textContent);
  var sourcesEl = document.getElementById("seer-sources");
  var sources = sourcesEl
    ? JSON.parse(sourcesEl.textContent)
    : { files: {}, spans: {} };
  var canvas = document.getElementById("canvas");
  var ctx = canvas.getContext("2d");
  var detail = document.getElementById("detail");
  var code = document.getElementById("code");
  var select = document.getElementById("entry");
  var NODE_W = 228;
  var NODE_H = 52;
  var HEADER = 44;
  var INSET = 14;
  var GAP = 12;
  var TOGGLE_W = 22;
  var MAX_INNER = NODE_W * 5 + GAP * 4;
  var LEVEL_FIRST = [44, 58, 86];
  var LEVEL_LAST = [18, 20, 28];
  var CYCLE_FILL = [42, 36, 22];
  var CYCLE_STROKE = [192, 163, 110];
  var collapsed = {};
  var filterKeep = {};

  var fnIds = {};
  data.nodes.forEach(function (n) {
    if (n.kind === "function") {
      fnIds[n.id] = n;
    }
  });

  function labelOf(n) {
    return n.file + ":" + n.line + " " + n.name;
  }

  function utf8Slice(str, start, end) {
    var bytes = new TextEncoder().encode(str);
    return new TextDecoder("utf-8").decode(bytes.subarray(start, end));
  }

  function dedent(s) {
    var lines = s.split("\n");
    while (lines.length && lines[lines.length - 1].trim() === "") {
      lines.pop();
    }
    var ind = null;
    lines.forEach(function (line) {
      if (!line.trim()) {
        return;
      }
      var n = line.match(/^[ \t]*/)[0].length;
      if (ind === null || n < ind) {
        ind = n;
      }
    });
    if (!ind) {
      return lines.join("\n");
    }
    return lines
      .map(function (line) {
        return line.slice(ind);
      })
      .join("\n");
  }

  function showCode(n) {
    detail.textContent = labelOf(n);
    var span = sources.spans && sources.spans[n.id];
    var src = sources.files && sources.files[n.file];
    if (!span || src == null) {
      code.textContent = "";
      return;
    }
    code.textContent = dedent(utf8Slice(src, span[0], span[1]));
  }

  function reachable(start) {
    var seen = {};
    var q = [start];
    seen[start] = true;
    while (q.length) {
      var id = q.shift();
      for (var i = 0; i < data.edges.length; i++) {
        var e = data.edges[i];
        if (e.from === id && fnIds[e.to] && !seen[e.to]) {
          seen[e.to] = true;
          q.push(e.to);
        }
      }
    }
    return seen;
  }

  function hasFnCallee(id) {
    for (var i = 0; i < data.edges.length; i++) {
      var e = data.edges[i];
      if (e.from === id && fnIds[e.to]) {
        return true;
      }
    }
    return false;
  }

  function isUsefulEntry(n) {
    return n.kind === "function" && n.entry && hasFnCallee(n.id);
  }

  data.nodes.forEach(function (n) {
    if (isUsefulEntry(n)) {
      var opt = document.createElement("option");
      opt.value = n.id;
      opt.textContent = labelOf(n);
      select.appendChild(opt);
    }
  });

  function usefulEntryIds() {
    var ids = [];
    data.nodes.forEach(function (n) {
      if (isUsefulEntry(n)) {
        ids.push(n.id);
      }
    });
    return ids;
  }

  function toggleRect(p) {
    return {
      x: p.x + p.w - TOGGLE_W - 6,
      y: p.y + 10,
      w: TOGGLE_W,
      h: 24,
    };
  }

  function pointInRect(pt, r) {
    return pt.x >= r.x && pt.x <= r.x + r.w && pt.y >= r.y && pt.y <= r.y + r.h;
  }

  function orderedCallees(fromId, keep) {
    var edges = data.edges
      .filter(function (e) {
        return e.from === fromId && keep[e.to];
      })
      .sort(function (a, b) {
        return a.seq - b.seq;
      });
    var seen = {};
    var out = [];
    edges.forEach(function (e) {
      if (!seen[e.to]) {
        seen[e.to] = true;
        out.push(e.to);
      }
    });
    return out;
  }

  function defaultEntry() {
    var i;
    var n;
    for (i = 0; i < data.nodes.length; i++) {
      n = data.nodes[i];
      if (
        isUsefulEntry(n) &&
        n.name === "main" &&
        /(^|\/)main\.(rs|java|ts|tsx|mts|cts)$/.test(n.file)
      ) {
        return n.id;
      }
    }
    for (i = 0; i < data.nodes.length; i++) {
      n = data.nodes[i];
      if (isUsefulEntry(n) && n.name === "main") {
        return n.id;
      }
    }
    return "";
  }

  function offsetBox(b, dx, dy) {
    b.x += dx;
    b.y += dy;
    b.children.forEach(function (c) {
      offsetBox(c, dx, dy);
    });
  }

  function layoutBox(id, ancestors, keep) {
    var node = fnIds[id];
    var path = ancestors.concat([id]);
    var key = path.join("\0");
    var isCycle = ancestors.indexOf(id) !== -1;
    var kids = isCycle ? [] : orderedCallees(id, keep);
    var canExpand = kids.length > 0;
    var isOpen = canExpand && !collapsed[key];
    var box = {
      id: id,
      key: key,
      node: node,
      x: 0,
      y: 0,
      w: NODE_W,
      h: NODE_H,
      canExpand: canExpand,
      isOpen: isOpen,
      isCycle: isCycle,
      depth: ancestors.length,
      children: [],
    };
    if (!isOpen) {
      return box;
    }
    var packed = [];
    var rowX = 0;
    var rowY = 0;
    var rowH = 0;
    var innerW = 0;
    var innerH = 0;
    kids.forEach(function (kid) {
      var child = layoutBox(kid, path, keep);
      if (rowX > 0 && rowX + child.w > MAX_INNER) {
        rowX = 0;
        rowY += rowH + GAP;
        rowH = 0;
      }
      offsetBox(child, rowX, rowY);
      packed.push(child);
      rowX += child.w + GAP;
      rowH = Math.max(rowH, child.h);
      innerW = Math.max(innerW, rowX - GAP);
      innerH = rowY + rowH;
    });
    packed.forEach(function (child) {
      offsetBox(child, INSET, HEADER + INSET);
    });
    box.children = packed;
    box.w = Math.max(NODE_W, innerW + INSET * 2);
    box.h = HEADER + innerH + INSET * 2;
    return box;
  }

  function flatten(b, out) {
    out.push(b);
    b.children.forEach(function (c) {
      flatten(c, out);
    });
  }

  var pan = { x: 16, y: 16, k: 1 };
  var boxes = [];
  var dpr = 1;
  var hover = null;
  var selected = null;
  var scene = { roots: [] };

  function sizeCanvas() {
    dpr = window.devicePixelRatio || 1;
    var w = canvas.clientWidth;
    var h = canvas.clientHeight;
    canvas.width = Math.max(1, Math.floor(w * dpr));
    canvas.height = Math.max(1, Math.floor(h * dpr));
  }

  function worldFromEvent(ev) {
    var rect = canvas.getBoundingClientRect();
    return {
      x: (ev.clientX - rect.left - pan.x) / pan.k,
      y: (ev.clientY - rect.top - pan.y) / pan.k,
    };
  }

  function hit(pt) {
    for (var i = boxes.length - 1; i >= 0; i--) {
      var b = boxes[i];
      if (pt.x >= b.x && pt.x <= b.x + b.w && pt.y >= b.y && pt.y <= b.y + b.h) {
        return b;
      }
    }
    return null;
  }

  function roundRect(x, y, w, h, r) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }

  function drawToggle(p, isOpen) {
    var r = toggleRect(p);
    var cx = r.x + r.w / 2;
    var cy = r.y + r.h / 2;
    ctx.fillStyle = "#8b93a7";
    ctx.beginPath();
    if (isOpen) {
      ctx.moveTo(cx - 5, cy - 3);
      ctx.lineTo(cx + 5, cy - 3);
      ctx.lineTo(cx, cy + 5);
    } else {
      ctx.moveTo(cx - 4, cy - 5);
      ctx.lineTo(cx + 5, cy);
      ctx.lineTo(cx - 4, cy + 5);
    }
    ctx.closePath();
    ctx.fill();
  }

  function drawCycleMark(p) {
    var r = toggleRect(p);
    ctx.font = "16px ui-sans-serif, system-ui, sans-serif";
    ctx.fillStyle = "#e0af68";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("↩", r.x + r.w / 2, r.y + r.h / 2 + 1);
    ctx.textAlign = "start";
    ctx.textBaseline = "alphabetic";
  }

  function graphBounds() {
    var minX = Infinity;
    var minY = Infinity;
    var maxX = -Infinity;
    boxes.forEach(function (p) {
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x + p.w);
    });
    return {
      minX: minX,
      minY: minY,
      w: Math.max(1, maxX - minX),
    };
  }

  function widthFitK() {
    if (!boxes.length) {
      return 1;
    }
    return canvas.clientWidth / (graphBounds().w + 48);
  }

  function clampK(k) {
    var maxK = widthFitK();
    return Math.min(maxK, Math.max(Math.min(0.12, maxK), k));
  }

  function zoomAt(next, cx, cy) {
    next = clampK(next);
    if (next === pan.k) {
      return;
    }
    pan.x = cx - ((cx - pan.x) * next) / pan.k;
    pan.y = cy - ((cy - pan.y) * next) / pan.k;
    pan.k = next;
    paint();
  }

  function zoomBy(factor) {
    zoomAt(pan.k * factor, canvas.clientWidth / 2, canvas.clientHeight / 2);
  }

  function fit() {
    if (!boxes.length) {
      return;
    }
    var b = graphBounds();
    var k = widthFitK();
    pan.k = k;
    pan.x = (canvas.clientWidth - k * b.w) / 2 - k * b.minX;
    pan.y = 24 - k * b.minY;
  }

  function lerp(a, b, t) {
    return a + (b - a) * t;
  }

  function lerpRgb(c0, c1, t) {
    return [
      Math.round(lerp(c0[0], c1[0], t)),
      Math.round(lerp(c0[1], c1[1], t)),
      Math.round(lerp(c0[2], c1[2], t)),
    ];
  }

  function cssRgb(c) {
    return "rgb(" + c[0] + "," + c[1] + "," + c[2] + ")";
  }

  function paint() {
    var w = canvas.clientWidth;
    var h = canvas.clientHeight;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "#0e1016";
    ctx.fillRect(0, 0, w, h);
    ctx.translate(pan.x, pan.y);
    ctx.scale(pan.k, pan.k);
    var maxDepth = 0;
    boxes.forEach(function (p) {
      if (p.depth > maxDepth) {
        maxDepth = p.depth;
      }
    });
    boxes.forEach(function (p) {
      var n = p.node;
      var isSel = selected && selected.key === p.key;
      var isHov = hover && hover.key === p.key;
      var t = maxDepth === 0 ? 0 : p.depth / maxDepth;
      var fill = lerpRgb(LEVEL_FIRST, LEVEL_LAST, t);
      if (p.isCycle) {
        fill = lerpRgb(fill, CYCLE_FILL, 0.55);
      }
      if (isHov) {
        fill = lerpRgb(fill, [80, 96, 128], 0.28);
      }
      if (isSel) {
        fill = lerpRgb(fill, [70, 110, 180], 0.35);
      }
      ctx.fillStyle = cssRgb(fill);
      roundRect(p.x, p.y, p.w, p.h, 8);
      ctx.fill();
      var stroke = p.isCycle
        ? CYCLE_STROKE
        : lerpRgb([122, 162, 247], [47, 51, 66], t);
      ctx.strokeStyle = isSel ? (p.isCycle ? "#e0af68" : "#7aa2f7") : cssRgb(stroke);
      ctx.lineWidth = isSel || p.depth === 0 || p.isCycle ? 1.6 : 1;
      ctx.setLineDash(p.isCycle ? [5, 4] : []);
      ctx.stroke();
      ctx.setLineDash([]);
      var loc = n.file + ":" + n.line;
      if (loc.length > 28) {
        loc = loc.slice(0, 27) + "…";
      }
      ctx.font = "11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";
      ctx.fillStyle = p.isCycle ? "#c0a36e" : "#8b93a7";
      ctx.fillText(loc, p.x + 12, p.y + 18);
      ctx.font = "600 13px ui-sans-serif, system-ui, sans-serif";
      ctx.fillStyle = "#e8eaed";
      ctx.fillText(n.name, p.x + 12, p.y + 36);
      if (p.isCycle) {
        drawCycleMark(p);
      } else if (p.canExpand && p.depth > 0) {
        drawToggle(p, p.isOpen);
      }
    });
  }

  function relayout(keep, shouldFit) {
    if (!keep) {
      keep = {};
      usefulEntryIds().forEach(function (id) {
        keep[id] = true;
        var more = reachable(id);
        Object.keys(more).forEach(function (k) {
          keep[k] = true;
        });
      });
    }
    filterKeep = keep;
    var roots = [];
    if (select.value && fnIds[select.value]) {
      roots = [layoutBox(select.value, [], keep)];
    } else {
      var y = 0;
      usefulEntryIds().forEach(function (id) {
        var box = layoutBox(id, [], keep);
        offsetBox(box, 0, y);
        roots.push(box);
        y += box.h + GAP * 2;
      });
    }
    scene.roots = roots;
    boxes = [];
    roots.forEach(function (r) {
      flatten(r, boxes);
    });
    hover = null;
    if (selected) {
      var still = null;
      boxes.forEach(function (b) {
        if (b.key === selected.key) {
          still = b;
        }
      });
      selected = still;
    }
    sizeCanvas();
    if (shouldFit !== false) {
      fit();
    } else {
      pan.k = clampK(pan.k);
    }
    paint();
  }

  var drag = null;
  canvas.addEventListener("pointerdown", function (ev) {
    if (ev.button !== 0) {
      return;
    }
    var pt = worldFromEvent(ev);
    var b = hit(pt);
    if (b && b.canExpand && b.depth > 0 && pointInRect(pt, toggleRect(b))) {
      collapsed[b.key] = !collapsed[b.key];
      relayout(filterKeep, false);
      return;
    }
    if (b) {
      selected = b;
      showCode(b.node);
      paint();
    }
    drag = { x: ev.clientX, y: ev.clientY, px: pan.x, py: pan.y, moved: false };
    canvas.setPointerCapture(ev.pointerId);
  });
  canvas.addEventListener("pointermove", function (ev) {
    if (!drag) {
      var b = hit(worldFromEvent(ev));
      var next = b ? b.key : null;
      var prev = hover ? hover.key : null;
      canvas.style.cursor = b ? "pointer" : "grab";
      if (next !== prev) {
        hover = b;
        paint();
      }
      return;
    }
    var dx = ev.clientX - drag.x;
    var dy = ev.clientY - drag.y;
    if (!drag.moved && dx * dx + dy * dy < 16) {
      return;
    }
    drag.moved = true;
    canvas.style.cursor = "grabbing";
    pan.x = drag.px + dx;
    pan.y = drag.py + dy;
    paint();
  });
  canvas.addEventListener("pointerup", function () {
    drag = null;
    canvas.style.cursor = hover ? "pointer" : "grab";
  });
  canvas.addEventListener("pointerleave", function () {
    if (hover) {
      hover = null;
      paint();
    }
  });
  canvas.addEventListener(
    "wheel",
    function (ev) {
      ev.preventDefault();
      var dy = ev.deltaY;
      if (ev.deltaMode === 1) {
        dy *= 16;
      } else if (ev.deltaMode === 2) {
        dy *= canvas.clientHeight;
      }
      var factor = Math.exp(-dy * 0.003);
      if (factor > 0.96 && factor < 1.04) {
        factor = dy < 0 ? 1.1 : 0.9;
      }
      var rect = canvas.getBoundingClientRect();
      zoomAt(pan.k * factor, ev.clientX - rect.left, ev.clientY - rect.top);
    },
    { passive: false }
  );
  document.getElementById("zoom-in").addEventListener("click", function () {
    zoomBy(1.25);
  });
  document.getElementById("zoom-out").addEventListener("click", function () {
    zoomBy(0.8);
  });
  document.getElementById("zoom-fit").addEventListener("click", function () {
    fit();
    paint();
  });
  window.addEventListener("keydown", function (ev) {
    var tag = (ev.target && ev.target.tagName) || "";
    if (tag === "SELECT" || tag === "INPUT" || tag === "TEXTAREA") {
      return;
    }
    if (ev.key === "+" || ev.key === "=") {
      ev.preventDefault();
      zoomBy(1.25);
    } else if (ev.key === "-" || ev.key === "_") {
      ev.preventDefault();
      zoomBy(0.8);
    } else if (ev.key === "0") {
      ev.preventDefault();
      fit();
      paint();
    }
  });
  window.addEventListener("resize", function () {
    sizeCanvas();
    pan.k = clampK(pan.k);
    paint();
  });

  select.addEventListener("change", function () {
    collapsed = {};
    if (!select.value) {
      relayout(null);
    } else {
      relayout(reachable(select.value));
    }
  });
  var start = defaultEntry();
  if (start) {
    select.value = start;
    relayout(reachable(start));
  } else {
    relayout(null);
  }
  if (start && boxes.length) {
    selected = boxes[0];
    showCode(fnIds[start]);
    paint();
  }
})();
