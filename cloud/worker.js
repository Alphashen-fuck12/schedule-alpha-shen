/**
 * 周课表 云端同步 Worker
 * 方案A：Cloudflare Workers + KV（免费额度 10万请求/天）
 *
 * 职责：
 *  - GET /?t=<授权码>      手机扫码入口：校验授权码 -> 下发7天会话cookie -> 返回手机课表页面
 *  - GET /api/state        手机/电脑读取课表数据（需会话cookie 或 X-Read-Token）
 *  - POST /api/state       电脑端写入课表数据（需 X-Write-Token）
 *  - OPTIONS               CORS 预检（电脑端跨域调用）
 *
 * KV 键设计：
 *  - state            -> 课表数据 JSON 字符串（整个 elec_schedule_full_v1 结构）
 *  - auth:<码>        -> 授权码，value=过期时间戳(ms)
 *  - session:<sid>    -> 会话，value=过期时间戳(ms)
 *
 * 环境变量（部署时在 Worker 设置里配）：
 *  - WRITE_TOKEN      -> 电脑端写密钥（首次部署时生成随机串，填进电脑端设置）
 *  - SESSION_DAYS     -> 会话有效天数，默认 7
 *  - AUTH_MINUTES     -> 授权码有效期分钟，默认 30
 */

const SESSION_DAYS = parseInt(SESSION_DAYS || '7', 10);
const AUTH_MINUTES = parseInt(AUTH_MINUTES || '30', 10);
const STATE_KEY = 'state';

const CORS_HEADERS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET,POST,OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type, X-Write-Token, X-Read-Token',
  'Access-Control-Max-Age': '86400',
};

function json(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json; charset=utf-8', ...CORS_HEADERS },
  });
}

function html(content, status = 200) {
  return new Response(content, {
    status,
    headers: { 'Content-Type': 'text/html; charset=utf-8', ...CORS_HEADERS },
  });
}

function genToken(len = 32) {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  let s = '';
  const arr = new Uint8Array(len);
  crypto.getRandomValues(arr);
  for (let i = 0; i < len; i++) s += chars[arr[i] % chars.length];
  return s;
}

// 从 cookie 头解析指定 cookie
function readCookie(header, name) {
  if (!header) return null;
  const m = header.match(new RegExp('(?:^|;\\s*)' + name + '=([^;]+)'));
  return m ? m[1] : null;
}

// 会话是否有效
async function sessionValid(env, sid) {
  if (!sid) return false;
  const exp = await env.SCHEDULE_KV.get('session:' + sid);
  if (!exp) return false;
  if (parseInt(exp, 10) < Date.now()) {
    await env.SCHEDULE_KV.delete('session:' + sid);
    return false;
  }
  return true;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname;
    const method = request.method;

    // CORS 预检
    if (method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    // ---------- 电脑端：注册一次性授权码（生成动态二维码用） ----------
    if (method === 'POST' && path === '/api/auth') {
      if (!env.WRITE_TOKEN) return json({ ok: false, error: 'server_not_configured' }, 500);
      const auth = request.headers.get('X-Write-Token') || '';
      if (auth !== env.WRITE_TOKEN) return json({ ok: false, error: 'bad_write_token' }, 401);
      let body;
      try { body = await request.json(); } catch { return json({ ok: false, error: 'bad_body' }, 400); }
      const code = String(body.code || '').trim();
      if (!/^[A-Za-z0-9]{8,24}$/.test(code)) return json({ ok: false, error: 'bad_code' }, 400);
      const exp = Date.now() + AUTH_MINUTES * 60000;
      await env.SCHEDULE_KV.put('auth:' + code, String(exp));
      return json({ ok: true, expires_in: AUTH_MINUTES * 60, ttl_minutes: AUTH_MINUTES });
    }

    // ---------- 电脑端：写入课表数据 ----------
    if (method === 'POST' && path === '/api/state') {
      if (!env.WRITE_TOKEN) return json({ ok: false, error: 'server_not_configured' }, 500);
      const auth = request.headers.get('X-Write-Token') || '';
      if (auth !== env.WRITE_TOKEN) return json({ ok: false, error: 'bad_write_token' }, 401);
      let body;
      try { body = await request.text(); } catch { return json({ ok: false, error: 'bad_body' }, 400); }
      if (!body || body.length > 5 * 1024 * 1024) return json({ ok: false, error: 'too_large' }, 400);
      await env.SCHEDULE_KV.put(STATE_KEY, body);
      return json({ ok: true, ts: Date.now() });
    }

    // ---------- 读取课表数据（手机/电脑均可，需会话或读密钥） ----------
    if (method === 'GET' && path === '/api/state') {
      const sid = readCookie(request.headers.get('Cookie'), 'sched_session');
      const readToken = request.headers.get('X-Read-Token') || '';
      const okSession = await sessionValid(env, sid);
      const okRead = env.READ_TOKEN && readToken === env.READ_TOKEN;
      if (!okSession && !okRead) return json({ ok: false, error: 'unauthorized' }, 401);
      const data = await env.SCHEDULE_KV.get(STATE_KEY);
      if (data === null) return json({ ok: true, data: null, exists: false });
      return json({ ok: true, exists: true, data });
    }

    // ---------- 扫码入口：校验授权码，下发会话 cookie，返回手机页面 ----------
    if (method === 'GET' && (path === '/' || path === '/m')) {
      const t = url.searchParams.get('t');
      // 无授权码但已有有效会话 -> 直接放行
      const sid = readCookie(request.headers.get('Cookie'), 'sched_session');
      if (!t && sid && (await sessionValid(env, sid))) {
        return html(MOBILE_PAGE);
      }
      if (!t) {
        return html(`<meta charset="utf-8"><body style="font-family:sans-serif;text-align:center;padding-top:60px">
          <h3>课表二维码已失效</h3><p>请找电脑上的课表软件重新点一次「手机同步」，再扫码打开。</p></body>`);
      }
      const exp = await env.SCHEDULE_KV.get('auth:' + t);
      if (!exp || parseInt(exp, 10) < Date.now()) {
        return html(`<meta charset="utf-8"><body style="font-family:sans-serif;text-align:center;padding-top:60px">
          <h3>二维码已过期</h3><p>二维码有时效，请回电脑重新点「手机同步」生成新码。</p></body>`);
      }
      // 授权码有效：作废它（一次性），发新会话
      await env.SCHEDULE_KV.delete('auth:' + t);
      const sidNew = genToken(32);
      await env.SCHEDULE_KV.put('session:' + sidNew, String(Date.now() + SESSION_DAYS * 86400000));
      const cookie = `sched_session=${sidNew}; Path=/; Max-Age=${SESSION_DAYS * 86400}; SameSite=Lax`;
      const resp = html(MOBILE_PAGE);
      resp.headers.append('Set-Cookie', cookie);
      return resp;
    }

    // 其它 -> 404
    return json({ ok: false, error: 'not_found' }, 404);
  },
};

/**
 * 手机端专用课表页面（内嵌，与 Worker 同源，天然避免 CORS）
 * 只读展示，数据缓存到手机 localStorage，进入时后台静默刷新。
 */
const MOBILE_PAGE = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no">
<title>电子记课表</title>
<style>
  *{box-sizing:border-box;margin:0;padding:0;-webkit-tap-highlight-color:transparent}
  body{font-family:-apple-system,"PingFang SC","Microsoft YaHei",sans-serif;background:#f3f6fb;color:#222;padding-bottom:80px}
  header{background:linear-gradient(135deg,#4a7dff,#6a5cff);color:#fff;padding:18px 16px 14px}
  header h1{font-size:20px;font-weight:700}
  header .sub{font-size:12px;opacity:.85;margin-top:4px}
  .weeks{display:flex;gap:6px;overflow-x:auto;padding:10px 12px;background:#fff;border-bottom:1px solid #e6ebf3;position:sticky;top:0;z-index:5}
  .weeks .w{flex:0 0 auto;padding:7px 14px;border-radius:18px;background:#eef2fa;font-size:13px;color:#556;cursor:pointer;white-space:nowrap}
  .weeks .w.on{background:#4a7dff;color:#fff;font-weight:600}
  .list{padding:10px 12px}
  .day{margin-bottom:14px}
  .day .dh{font-size:14px;font-weight:700;color:#4a7dff;margin:8px 2px}
  .cell{background:#fff;border-radius:12px;padding:11px 13px;margin:6px 0;box-shadow:0 1px 3px rgba(20,40,90,.06);border-left:4px solid #4a7dff}
  .cell .cname{font-size:16px;font-weight:700}
  .cell .cinfo{font-size:12px;color:#8894ab;margin-top:4px;display:flex;flex-wrap:wrap;gap:4px 10px}
  .cell.holiday{border-left-color:#f0a020;background:#fffbf0}
  .empty{text-align:center;color:#aab3c5;font-size:13px;padding:20px 0}
  .footer{position:fixed;bottom:0;left:0;right:0;background:#fff;border-top:1px solid #e6ebf3;padding:8px 14px;display:flex;justify-content:space-between;align-items:center;font-size:12px;color:#8894ab}
  .footer .dot{width:8px;height:8px;border-radius:50%;background:#8fd17b;display:inline-block;margin-right:5px}
  .footer .dot.off{background:#e2695c}
  #toast{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);background:rgba(0,0,0,.75);color:#fff;padding:10px 18px;border-radius:10px;font-size:13px;display:none;z-index:99}
</style>
</head>
<body>
<header>
  <h1>📖 电子记课表</h1>
  <div class="sub" id="sub">正在读取课表…</div>
</header>
<div class="weeks" id="weeks"></div>
<div class="list" id="list"><div class="empty">加载中…</div></div>
<div class="footer">
  <span><span class="dot" id="dot"></span><span id="online">同步中</span></span>
  <span id="ts"></span>
</div>
<div id="toast"></div>
<script>
var KEY='elec_schedule_mobile_v1';
var state=null, weekIdx=0, cache=null;

function $(s){return document.querySelector(s)}

function toast(msg){var t=$('#toast');t.textContent=msg;t.style.display='block';setTimeout(function(){t.style.display='none'},1600)}

// 周次计算：与桌面端一致，用 week1（第1周周一日期）算今天第几周
function weekNumber(){
  var s=state||{};
  if(s.settings&&s.settings.week1){
    var d=new Date();var w1=new Date(s.settings.week1);
    var diff=Math.floor((d.getTime()-w1.getTime())/86400000);
    return diff>=0?Math.floor(diff/7)+1:1;
  }
  return 1;
}

function buildWeeks(){
  var s=state||{};var n=(s.settings&&s.settings.weekCount)||5;
  var h='';for(var i=1;i<=n;i++){h+='<div class="w'+(i===weekIdx+1?' on':'')+'" data-i="'+(i-1)+'">第'+cn(i)+'周</div>'}
  $('#weeks').innerHTML=h;
  var ws=$('#weeks').querySelectorAll('.w');
  for(var k=0;k<ws.length;k++){ws[k].addEventListener('click',function(){weekIdx=parseInt(this.getAttribute('data-i'),10);render()})}
}
function cn(n){var a=['一','二','三','四','五','六','七','八','九','十'];return n<=10?a[n-1]:n}

function render(){
  buildWeeks();
  var s=state||{};
  var week=String(weekIdx+1);
  var grid=(s.grid&&s.grid[week])||{};
  var days=[1,2,3,4,5,6,7];
  var wdays=['周一','周二','周三','周四','周五','周六','周日'];
  var holidays=s.holidays||{};
  var h='';
  days.forEach(function(d){
    var dh=wdays[d-1]+'';
    var cells=grid[d]||[];
    h+='<div class="day"><div class="dh">'+dh+'</div>';
    if(cells.length===0){h+='<div class="empty">当天没课</div>'}
    cells.forEach(function(c){
      var isH=holidays[week]&&holidays[week][d];
      h+='<div class="cell'+(isH?' holiday':'')+'">'+
         '<div class="cname">'+(c.name||c.subject||'')+'</div>'+
         '<div class="cinfo">'+(c.teacher?('<span>👩‍🏫 '+c.teacher+'</span>'):'')+
         (c.room?('<span>🏫 '+c.room+'</span>'):'')+
         (c.range?('<span>周次 '+c.range+'</span>'):'')+'</div></div>';
    });
    h+='</div>';
  });
  $('#list').innerHTML=h;
  var today=new Date().getDay(); // 0=日
  if(today===0)today=7;
  var dhs=$('#list').querySelectorAll('.day .dh');
  if(dhs[today-1]){dhs[today-1].style.color='#e2603f'}
}

function applyData(raw){
  var obj=null;
  try{obj=JSON.parse(raw)}catch(e){}
  if(!obj){$('#list').innerHTML='<div class="empty">云端还没有课表数据</div>';$('#sub').textContent='未同步到数据';return}
  state=obj;cache=raw;weekIdx=Math.max(0,weekNumber()-1);
  try{localStorage.setItem(KEY,raw)}catch(e){}
  $('#sub').textContent='课表已同步 · 老师改课会自动更新';
  render();
}

function loadLocal(){
  try{var raw=localStorage.getItem(KEY);if(raw){applyData(raw);return true}}catch(e){}
  return false;
}

function fetchCloud(){
  var x=new XMLHttpRequest();
  x.open('GET','/api/state',true);
  x.withCredentials=false;
  x.onload=function(){
    var online=$('#online'),dot=$('#dot');
    if(x.status===200){
      var r=JSON.parse(x.responseText);
      online.textContent='在线';dot.className='dot';
      if(r.exists){if(r.data!==cache){applyData(r.data)}}
      var d=new Date();$('#ts').textContent='更新 '+pad(d.getHours())+':'+pad(d.getMinutes());
    }else{
      online.textContent='离线';dot.classList.add('off');
    }
  };
  x.onerror=function(){var o=$('#online');o.textContent='离线';$('#dot').classList.add('off')};
  x.send();
}
function pad(n){return n<10?'0'+n:''+n}

// 启动：先用本地缓存秒开，再后台拉云端
var hasLocal=loadLocal();
fetchCloud();
if(hasLocal)setInterval(fetchCloud,30000); // 30秒心跳保活
</script>
</body>
</html>`;
