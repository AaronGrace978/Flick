"use strict";
const canvas = document.querySelector("#warp"),
  particles = document.querySelector("#particles"),
  card = document.querySelector("#asset"),
  notice = document.querySelector("#notice");
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
const gl = canvas.getContext("webgl", {
    alpha: true,
    premultipliedAlpha: true,
    antialias: false,
    depth: false,
  }),
  ctx = particles.getContext("2d");
const vertex = `attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}`;
const fragment = `precision highp float;
uniform vec2 resolution;uniform float angle;uniform float power;uniform float splash;
float bezier(float t,float a,float b,float c,float d){float u=1.-t;return u*u*u*a+3.*u*u*t*b+3.*u*t*t*c+t*t*t*d;}
void main(){vec2 uv=gl_FragCoord.xy/resolution;vec2 outwards=vec2(cos(angle),sin(angle));vec2 edge=.5+outwards*(.5/max(abs(outwards.x),abs(outwards.y)));vec2 metric=vec2(resolution.x/resolution.y,1.);vec2 normal=normalize(-outwards*metric);vec2 tangent=vec2(-normal.y,normal.x);vec2 p=(uv-edge)*metric;float depth=dot(p,normal);float side=dot(p,tangent);float bell=exp(-side*side/.018);float amp=power*.18;float t=clamp((side+.3)/.6,0.,1.);float curve=bezier(t,0.,amp*1.34,amp*1.34,0.);float band=exp(-pow((depth-curve*bell)/(.002+power*.006),2.));float halo=exp(-pow((depth-curve*bell)/.038,2.))*.25;float grid=pow(.5+.5*cos(side*520.+sin(depth*100.)*.3),14.);float mesh=exp(-pow((depth-curve*bell*.7)/.014,2.))*grid*.34;float ring=exp(-pow((length(p)-splash*.4)/.014,2.))*(1.-splash)*step(.001,splash);float centerMask=smoothstep(.08,.43,length((uv-.5)*metric));float alpha=min(.4,((band+halo+mesh)*bell*power+ring*.8)*centerMask);vec3 cyan=mix(vec3(0.,.94,1.),vec3(.77,1.,1.),band*.5+ring*.3);gl_FragColor=vec4(cyan*alpha,alpha);}`;
let program, uniforms;
function compile(type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS))
    throw Error(gl.getShaderInfoLog(shader));
  return shader;
}
function initGL() {
  if (!gl) return;
  program = gl.createProgram();
  gl.attachShader(program, compile(gl.VERTEX_SHADER, vertex));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragment));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS))
    throw Error(gl.getProgramInfoLog(program));
  gl.useProgram(program);
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW,
  );
  const p = gl.getAttribLocation(program, "p");
  gl.enableVertexAttribArray(p);
  gl.vertexAttribPointer(p, 2, gl.FLOAT, false, 0, 0);
  uniforms = Object.fromEntries(
    ["resolution", "angle", "power", "splash"].map((key) => [
      key,
      gl.getUniformLocation(program, key),
    ]),
  );
}
let width = innerWidth,
  height = innerHeight;
function resize() {
  width = innerWidth;
  height = innerHeight;
  const ratio = Math.min(devicePixelRatio || 1, 2);
  for (const surface of [canvas, particles]) {
    surface.width = Math.round(width * ratio);
    surface.height = Math.round(height * ratio);
  }
  if (gl) gl.viewport(0, 0, canvas.width, canvas.height);
  if (ctx) ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
}
let telemetry = { distance: 0.65, velocity: 0, angle: Math.PI, phase: "idle" },
  lastTelemetry = 0,
  power = 0,
  splashAt = -10000,
  specks = [],
  animation = null,
  noticeUntil = 0;
function edgePoint(angle) {
  const x = Math.cos(angle),
    y = -Math.sin(angle),
    scale = 0.5 / Math.max(Math.abs(x), Math.abs(y));
  return { x: (0.5 + x * scale) * width, y: (0.5 + y * scale) * height };
}
function capture(angle) {
  splashAt = performance.now();
  if (reduced) return;
  const edge = edgePoint(angle);
  specks = Array.from({ length: 42 }, (_, i) => ({
    x: edge.x,
    y: edge.y,
    vx:
      -Math.cos(angle) * (95 + Math.random() * 260) +
      (Math.random() - 0.5) * 150,
    vy:
      Math.sin(angle) * (95 + Math.random() * 260) +
      (Math.random() - 0.5) * 150,
    life: 1,
    rate: 0.5 + Math.random(),
    size: i % 7 === 0 ? 2.2 : 1,
  }));
}
function receiveTelemetry(data) {
  if (![data.distance, data.velocity, data.angle].every(Number.isFinite))
    return;
  telemetry = data;
  lastTelemetry = performance.now();
  if (data.phase === "capture") capture(data.angle);
}
function receiveAsset(data) {
  if (
    !Number.isSafeInteger(data.id) ||
    typeof data.label !== "string" ||
    !Number.isFinite(data.angle) ||
    (animation && !animation.completed)
  )
    return;
  document.querySelector("#asset-name").textContent = data.label;
  document.querySelector("#number").textContent = String(data.id).padStart(
    2,
    "0",
  );
  document.querySelector("#asset-state").textContent = "Crossing the boundary";
  document.querySelector("#channel-state").textContent = "CAPTURED";
  card.className = "";
  const edge = edgePoint(data.angle),
    inward = { x: -Math.cos(data.angle), y: Math.sin(data.angle) };
  const end = {
    x: Math.max(18, Math.min(width - 370, edge.x + inward.x * 230 - 176)),
    y: Math.max(18, Math.min(height - 220, edge.y + inward.y * 180 - 90)),
  };
  animation = {
    id: data.id,
    start: performance.now(),
    from: { x: edge.x - 176, y: edge.y - 90 },
    to: end,
    completed: false,
  };
  capture(data.angle);
}
function status(data) {
  if (!data.ok) {
    notice.textContent = data.message;
    noticeUntil = performance.now() + 7000;
  }
  if (animation && data.id === animation.id) {
    card.classList.add(data.ok ? "delivered" : "failed");
    document.querySelector("#channel-state").textContent = data.ok
      ? "DELIVERED"
      : "MANUAL PASTE";
    document.querySelector("#asset-state").textContent = data.ok
      ? "Paste sent to your workspace"
      : "Received · see desktop status";
  }
}
let rendered = false;
let previous = performance.now();
function frame(now) {
  const dt = Math.min((now - previous) / 1000, 0.05);
  previous = now;
  const fresh = now - lastTelemetry < 250;
  const target =
    fresh && telemetry.phase === "approach"
      ? Math.max(0, Math.min(1, (0.5 - telemetry.distance) / 0.35))
      : 0;
  power += (target - power) * Math.min(1, dt * 12);
  const splash = Math.max(0, Math.min(1, (now - splashAt) / 850));
  if (gl && program && !gl.isContextLost()) {
    if (power > 0.001 || (splash > 0 && splash < 1)) {
      gl.uniform2f(uniforms.resolution, canvas.width, canvas.height);
      gl.uniform1f(uniforms.angle, telemetry.angle);
      gl.uniform1f(uniforms.power, power);
      gl.uniform1f(uniforms.splash, splash < 1 ? splash : 0);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
      rendered = true;
    } else if (rendered) {
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      rendered = false;
    }
  }
  if (ctx && specks.length) {
    ctx.clearRect(0, 0, width, height);
    for (const p of specks) {
      p.x += p.vx * dt;
      p.y += p.vy * dt;
      p.vx *= Math.exp(-dt * 1.8);
      p.vy *= Math.exp(-dt * 1.8);
      p.life -= p.rate * dt;
      if (p.life > 0) {
        ctx.fillStyle = `rgba(105,250,255,${p.life * 0.8})`;
        ctx.fillRect(p.x, p.y, p.size, p.size);
      }
    }
  }
  if (specks.length && specks.every((p) => p.life <= 0)) specks = [];
  if (animation) {
    const age = now - animation.start,
      t = Math.min(1, age / 850),
      ease = 1 - Math.pow(1 - t, 4);
    const x = reduced
        ? animation.to.x
        : animation.from.x + (animation.to.x - animation.from.x) * ease,
      y = reduced
        ? animation.to.y
        : animation.from.y + (animation.to.y - animation.from.y) * ease;
    card.style.transform = `translate(${x}px,${y}px) scale(${0.94 + 0.06 * ease})`;
    card.style.opacity = String(
      Math.min(1, age / 150) * Math.max(0, Math.min(1, (4000 - age) / 500)),
    );
    card.querySelector(".progress").style.transform = `scaleX(${t})`;
    if (t === 1 && !animation.completed) {
      animation.completed = true;
      if (window.__TAURI__)
        window.__TAURI__.core
          .invoke("complete_handoff", { id: animation.id })
          .catch((error) =>
            status({ id: animation?.id, ok: false, message: String(error) }),
          );
    }
    if (age > 4000) {
      animation = null;
      card.style.opacity = "0";
    }
  }
  notice.style.opacity = now < noticeUntil ? "1" : "0";
  requestAnimationFrame(frame);
}
async function start() {
  try {
    initGL();
  } catch (error) {
    program = null;
    console.error(error);
    status({
      ok: false,
      message: "WebGL unavailable; handoff cards remain active.",
    });
  }
  resize();
  addEventListener("resize", resize);
  requestAnimationFrame(frame);
  if (window.__TAURI__) {
    await window.__TAURI__.event.listen("flick://telemetry", (event) =>
      receiveTelemetry(event.payload),
    );
    await window.__TAURI__.event.listen("flick://asset", (event) =>
      receiveAsset(event.payload),
    );
    await window.__TAURI__.event.listen("flick://status", (event) =>
      status(event.payload),
    );
    await window.__TAURI__.core.invoke("frontend_ready");
  } else if (new URLSearchParams(location.search).has("demo")) {
    let d = 0.65;
    const timer = setInterval(() => {
      d -= 0.012;
      receiveTelemetry({
        distance: d,
        velocity: 1,
        angle: Math.PI - 0.12,
        phase: "approach",
      });
      if (d < 0.2) {
        clearInterval(timer);
        receiveAsset({
          id: 1,
          label: "Orbit — concept.sketch",
          angle: Math.PI - 0.12,
        });
      }
    }, 35);
  }
}
start().catch((error) => console.error(error));
