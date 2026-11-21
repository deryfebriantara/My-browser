//! Script halaman untuk kontrol dari Claude. Setiap script menghasilkan satu
//! string JSON, berisi data atau `{"error": "..."}`.
//!
//! Elemen yang bisa diklik atau diisi diberi nomor oleh `list_elements`
//! (atribut `data-mb-ref`); `click`, `type_text`, dan `inspect_element`
//! memakai nomor itu.
//!
//! `run_js` dan `wait_for` adalah badan fungsi async (dijalankan lewat
//! `Page::evaluate_async`), sisanya ekspresi biasa (`Page::evaluate`).

use crate::control::{LogLevel, ScrollDirection, WaitState, YoutubeAction};

/// Pencatat console dan network untuk alat developer, dipasang di awal
/// setiap dokumen main frame.
pub const DEV_CAPTURE: &str = include_str!("dev_capture.js");

const NO_CAPTURE: &str = "JSON.stringify({error:'Pencatat developer belum ada di halaman ini \
(halaman dibuka sebelum browser diperbarui). Muat ulang halaman lalu coba lagi.'})";

/// Batas maksimum `timeout_ms` untuk `wait_for`.
pub const MAX_WAIT_MS: u64 = 30_000;

/// Batas bawaan panjang teks halaman yang dikirim ke Claude.
pub const DEFAULT_MAX_CHARS: usize = 20_000;

/// String Rust sebagai literal string JavaScript yang aman.
fn js_string(s: &str) -> String {
    // Literal JSON adalah literal JavaScript yang sah, kecuali U+2028/U+2029
    // pada mesin JavaScript lama, jadi keduanya di-escape juga.
    serde_json::to_string(s)
        .expect("string selalu bisa diserialisasi")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub fn read_page(max_chars: usize) -> String {
    format!(
        "(function(){{var t=document.body?document.body.innerText:'';\
         return JSON.stringify({{title:document.title,url:location.href,length:t.length,\
         truncated:t.length>{max_chars},text:t.slice(0,{max_chars})}});}})()"
    )
}

pub const LIST_ELEMENTS: &str = r#"(function(){
  var sel='a[href],button,input:not([type=hidden]),textarea,select,summary,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[contenteditable=""],[contenteditable=true]';
  document.querySelectorAll('[data-mb-ref]').forEach(function(e){e.removeAttribute('data-mb-ref');});
  var out=[],els=document.querySelectorAll(sel);
  for(var i=0;i<els.length&&out.length<300;i++){
    var e=els[i],r=e.getBoundingClientRect(),s=getComputedStyle(e);
    if(r.width===0||r.height===0||s.visibility==='hidden'||s.display==='none'||e.disabled)continue;
    var n=out.length+1;e.setAttribute('data-mb-ref',String(n));
    var label=(e.getAttribute('aria-label')||e.innerText||e.placeholder||e.title||e.getAttribute('alt')||e.name||'').trim().replace(/\s+/g,' ').slice(0,80);
    var item={element:n,tag:e.tagName.toLowerCase(),text:label};
    if(e.tagName==='INPUT')item.type=e.type;
    if(e.tagName==='INPUT'&&e.type!=='password'||e.tagName==='TEXTAREA'||e.tagName==='SELECT')item.value=String(e.value).slice(0,80);
    if(e.href)item.href=e.href;
    if(r.bottom<0||r.top>innerHeight)item.offscreen=true;
    out.push(item);
  }
  return JSON.stringify({count:out.length,elements:out});
})()"#;

fn find_element(element: u32) -> String {
    format!(
        "var e=document.querySelector('[data-mb-ref=\"{element}\"]');\
         if(!e)return JSON.stringify({{error:'Elemen {element} tidak ditemukan. Panggil list_elements lagi.'}});"
    )
}

pub fn click(element: u32) -> String {
    format!(
        "(function(){{{find}e.scrollIntoView({{block:'center'}});if(e.focus)e.focus();e.click();\
         return JSON.stringify({{clicked:{element},text:(e.innerText||e.value||e.getAttribute('aria-label')||'').trim().slice(0,80)}});}})()",
        find = find_element(element)
    )
}

pub fn type_text(element: u32, text: &str, submit: bool) -> String {
    format!(
        "(function(){{{find}var text={text};e.scrollIntoView({{block:'center'}});e.focus();\
         if(e.isContentEditable){{document.execCommand('selectAll');document.execCommand('insertText',false,text);}}\
         else{{var proto=e.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:e.tagName==='SELECT'?HTMLSelectElement.prototype:HTMLInputElement.prototype;\
         Object.getOwnPropertyDescriptor(proto,'value').set.call(e,text);\
         e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));}}\
         var submitted=false;if({submit}){{if(e.form&&e.form.requestSubmit){{e.form.requestSubmit();submitted=true;}}\
         else{{['keydown','keypress','keyup'].forEach(function(t){{e.dispatchEvent(new KeyboardEvent(t,{{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true}}));}});submitted=true;}}}}\
         return JSON.stringify({{typed:{element},submitted:submitted}});}})()",
        find = find_element(element),
        text = js_string(text),
    )
}

pub fn scroll(direction: ScrollDirection) -> String {
    let action = match direction {
        ScrollDirection::Up => "window.scrollBy(0,-innerHeight*0.8)",
        ScrollDirection::Down => "window.scrollBy(0,innerHeight*0.8)",
        ScrollDirection::Top => "window.scrollTo(0,0)",
        ScrollDirection::Bottom => "window.scrollTo(0,document.documentElement.scrollHeight)",
    };
    format!(
        "(function(){{{action};return JSON.stringify({{scroll_y:Math.round(scrollY),\
         max_scroll_y:Math.max(0,document.documentElement.scrollHeight-innerHeight)}});}})()"
    )
}

pub fn youtube(action: YoutubeAction, value: Option<f64>) -> String {
    let value = value.filter(|v| v.is_finite());
    let command = match (action, value) {
        (YoutubeAction::Status, _) => String::new(),
        (YoutubeAction::Play, _) => "p.playVideo();".into(),
        (YoutubeAction::Pause, _) => "p.pauseVideo();".into(),
        (YoutubeAction::Seek, Some(v)) => format!("p.seekTo({v},true);"),
        (YoutubeAction::Rate, Some(v)) => format!("p.setPlaybackRate({v});"),
        (YoutubeAction::Seek | YoutubeAction::Rate, None) => {
            return "JSON.stringify({error:'Aksi ini butuh value (detik untuk seek, kecepatan untuk rate).'})".into();
        }
    };
    format!(
        "(function(){{var p=document.getElementById('movie_player');\
         if(!p||!p.getPlayerState)return JSON.stringify({{error:'Tidak ada player YouTube di tab ini.'}});\
         {command}var states={{'-1':'belum mulai','0':'selesai','1':'diputar','2':'dijeda','3':'memuat','5':'siap'}};\
         var d=p.getVideoData?p.getVideoData():{{}};\
         return JSON.stringify({{title:d.title||document.title,state:states[String(p.getPlayerState())]||String(p.getPlayerState()),\
         time:Math.round(p.getCurrentTime()),duration:Math.round(p.getDuration()),rate:p.getPlaybackRate()}});}})()"
    )
}

/// Badan fungsi async: hasil ekspresi terakhir `code`; Promise ditunggu.
/// Ditolak di halaman yang kolom password-nya terisi, supaya password yang
/// baru diketik atau diisi pengelola password tidak bisa dibaca lewat run_js.
pub fn run_js(code: &str) -> String {
    format!(
        "if(Array.prototype.some.call(document.querySelectorAll('input[type=password]'),function(e){{return e.value;}}))\
         return JSON.stringify({{error:'Halaman ini punya kolom password yang terisi, jadi run_js diblokir supaya \
         password tidak terbaca. Kosongkan kolom password atau muat ulang halaman, lalu coba lagi.'}});\
         var r;try{{r=(0,eval)({code});if(r&&typeof r.then==='function')r=await r;}}\
         catch(e){{return JSON.stringify({{error:String(e)}});}}\
         try{{return JSON.stringify({{result:r===undefined?null:r}});}}catch(e){{return JSON.stringify({{result:String(r)}});}}",
        code = js_string(code)
    )
}

pub fn console_logs(level: LogLevel, since: u64, limit: usize) -> String {
    let min = match level {
        LogLevel::All => 0,
        LogLevel::Warn => 1,
        LogLevel::Error => 2,
    };
    format!(
        "(function(){{var d=window.__mbDev;if(!d)return {NO_CAPTURE};\
         var rank={{debug:0,log:0,info:0,warn:1,error:2}};\
         var list=d.logs.filter(function(e){{return rank[e.level]>={min}&&e.seq>{since};}});\
         var total=list.length;list=list.slice(-{limit});\
         return JSON.stringify({{url:location.href,total:total,returned:list.length,last_seq:d.seq,entries:list}});}})()"
    )
}

pub fn network_log(failed_only: bool, url_contains: Option<&str>, include_resources: bool, limit: usize) -> String {
    let contains = js_string(url_contains.unwrap_or(""));
    format!(
        "(function(){{var d=window.__mbDev;if(!d)return {NO_CAPTURE};var list=d.net.slice();\
         if({include_resources}){{performance.getEntriesByType('resource').forEach(function(r){{\
         if(r.initiatorType==='fetch'||r.initiatorType==='xmlhttprequest')return;\
         list.push({{type:r.initiatorType,url:r.name,start:Math.round(r.startTime),duration:Math.round(r.duration),\
         size:r.transferSize||undefined,status:r.responseStatus||undefined}});}});\
         list.sort(function(a,b){{return a.start-b.start;}});}}\
         if({failed_only})list=list.filter(function(e){{return e.error||e.status>=400;}});\
         var c={contains};if(c)list=list.filter(function(e){{return e.url.indexOf(c)>=0;}});\
         var total=list.length;list=list.slice(-{limit});\
         return JSON.stringify({{url:location.href,total:total,returned:list.length,entries:list,\
         note:d.local?undefined:'Isi respons hanya dicatat untuk situs lokal (localhost, IP lokal, .test).'}});}})()"
    )
}

/// Badan fungsi async yang menunggu elemen (atau teks) muncul, terlihat,
/// atau hilang, paling lama `timeout_ms`.
pub fn wait_for(selector: Option<&str>, text: Option<&str>, state: WaitState, timeout_ms: u64) -> String {
    if selector.is_none() && text.is_none() {
        return "return JSON.stringify({error:'wait_for butuh selector atau text.'});".into();
    }
    let state = match state {
        WaitState::Visible => "visible",
        WaitState::Present => "present",
        WaitState::Hidden => "hidden",
    };
    format!(
        "var sel={sel},txt={txt},state='{state}',timeout={timeout},start=Date.now();\
         function shown(e){{var r=e.getBoundingClientRect(),s=getComputedStyle(e);\
         return r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none';}}\
         function find(){{var els=sel?Array.prototype.slice.call(document.querySelectorAll(sel)):(document.body?[document.body]:[]);\
         if(txt!==null)els=els.filter(function(e){{return (e.innerText||e.textContent||'').indexOf(txt)>=0;}});return els;}}\
         while(true){{var els;try{{els=find();}}catch(e){{return JSON.stringify({{error:'Selector tidak valid: '+e.message}});}}\
         var ok=state==='present'?els.length>0:state==='hidden'?!els.some(shown):els.some(shown);\
         if(ok)return JSON.stringify({{found:state!=='hidden',state:state,count:els.length,waited_ms:Date.now()-start}});\
         if(Date.now()-start>=timeout)return JSON.stringify({{error:'Waktu habis setelah '+timeout+' ms: kondisi '+state+' belum terpenuhi \
         (elemen cocok: '+els.length+').'}});\
         await new Promise(function(r){{setTimeout(r,100);}});}}",
        sel = selector.map_or("null".into(), js_string),
        txt = text.map_or("null".into(), js_string),
        timeout = timeout_ms.min(MAX_WAIT_MS),
    )
}

/// Properti CSS bawaan untuk `inspect_element`.
const DEFAULT_STYLES: &[&str] = &[
    "display", "position", "top", "left", "width", "height", "margin", "padding", "box-sizing", "color",
    "background-color", "font-family", "font-size", "font-weight", "line-height", "text-align", "border",
    "border-radius", "opacity", "visibility", "overflow", "z-index", "flex-direction", "justify-content",
    "align-items", "gap", "grid-template-columns",
];

/// Fungsi JavaScript `inspect(e)` yang menghasilkan detail satu elemen.
/// Disisipkan di akhir script; deklarasi fungsi di-hoist.
fn inspect_fn(styles: Option<&[String]>) -> String {
    let names: Vec<String> = match styles {
        Some(list) => list.iter().filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).cloned().collect(),
        None => DEFAULT_STYLES.iter().map(|s| s.to_string()).collect(),
    };
    let names = serde_json::to_string(&names).expect("daftar string selalu bisa diserialisasi");
    format!(
        "function inspect(e){{var r=e.getBoundingClientRect(),s=getComputedStyle(e),styles={{}};\
         {names}.forEach(function(p){{styles[p]=s.getPropertyValue(p);}});\
         function name(x){{if(!x)return null;var n=x.tagName.toLowerCase();if(x.id)n+='#'+x.id;\
         if(typeof x.className==='string'&&x.className.trim())n+='.'+x.className.trim().split(/\\s+/).join('.');return n;}}\
         var html=e.outerHTML,t=(e.innerText||'').trim();\
         return {{element:name(e),parent:name(e.parentElement),children:e.children.length,\
         text:t.length>300?t.slice(0,300)+'…':t,\
         box:{{x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height)}},\
         visible:r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none',\
         in_viewport:r.bottom>0&&r.right>0&&r.top<innerHeight&&r.left<innerWidth,\
         styles:styles,outer_html:html.length>3000?html.slice(0,3000)+'…':html}};}}"
    )
}

pub fn inspect_element(selector: Option<&str>, element: Option<u32>, styles: Option<&[String]>) -> String {
    let query = match (selector, element) {
        (Some(sel), _) => js_string(sel),
        (None, Some(n)) => js_string(&format!("[data-mb-ref=\"{n}\"]")),
        (None, None) => return "JSON.stringify({error:'inspect_element butuh selector atau element.'})".into(),
    };
    format!(
        "(function(){{var q={query},e,count;try{{e=document.querySelector(q);count=document.querySelectorAll(q).length;}}\
         catch(x){{return JSON.stringify({{error:'Selector tidak valid: '+x.message}});}}\
         if(!e)return JSON.stringify({{error:'Elemen tidak ditemukan: '+q}});\
         var o=inspect(e);o.matches=count;return JSON.stringify(o);{inspect}}})()",
        inspect = inspect_fn(styles)
    )
}

/// Mode "Tunjuk Elemen ke Claude" (lihat `element_picker.js`).
pub const ELEMENT_PICKER: &str = include_str!("element_picker.js");

/// Membuang tanda "Terpilih" dari mode Tunjuk Elemen (tampil sebentar setelah
/// memilih) supaya tidak ikut terfoto. Overlay mode pilih yang masih aktif
/// dibiarkan.
const CLEAR_PICK_OVERLAY: &str =
    "if(!window.__mbPicker)document.querySelectorAll('[data-mb-overlay]').forEach(function(h){h.remove();});";

/// Detail elemen yang terakhir ditunjuk pengguna, plus selector unik dan
/// posisinya di dokumen (untuk dipotret).
pub fn picked_element(styles: Option<&[String]>) -> String {
    format!(
        "(function(){{{CLEAR_PICK_OVERLAY}var p=window.__mbPicked;\
         if(!p)return JSON.stringify({{error:'Belum ada elemen yang ditunjuk di tab ini. Minta pengguna memilih menu \
         Claude > Tunjuk Elemen ke Claude (Cmd+Shift+E) di my-browser, lalu klik elemennya.'}});\
         var e=p.el&&p.el.isConnected?p.el:null;\
         if(!e){{try{{e=document.querySelector(p.selector);}}catch(x){{}}}}\
         if(!e)return JSON.stringify({{error:'Elemen yang ditunjuk sudah tidak ada di halaman (halaman berubah). \
         Minta pengguna menunjuk lagi.'}});\
         var o=inspect(e),r=e.getBoundingClientRect();\
         o.selector=p.selector;o.url=location.href;o.picked_seconds_ago=Math.round((Date.now()-p.at)/1000);\
         o.document_box={{x:r.left+scrollX,y:r.top+scrollY,width:r.width,height:r.height}};\
         return JSON.stringify(o);{inspect}}})()",
        inspect = inspect_fn(styles)
    )
}

/// Posisi dan ukuran elemen di dokumen (poin CSS), untuk screenshot elemen.
pub fn element_box(selector: &str) -> String {
    format!(
        "(function(){{{CLEAR_PICK_OVERLAY}var q={q},e;try{{e=document.querySelector(q);}}\
         catch(x){{return JSON.stringify({{error:'Selector tidak valid: '+x.message}});}}\
         if(!e)return JSON.stringify({{error:'Elemen tidak ditemukan: '+q}});\
         var r=e.getBoundingClientRect();\
         if(r.width===0||r.height===0)return JSON.stringify({{error:'Elemen tidak terlihat (ukurannya nol): '+q}});\
         return JSON.stringify({{x:r.left+scrollX,y:r.top+scrollY,width:r.width,height:r.height}});}})()",
        q = js_string(selector)
    )
}

/// Ukuran seluruh dokumen (poin CSS), untuk screenshot halaman penuh.
pub fn document_size() -> String {
    format!(
        "(function(){{{CLEAR_PICK_OVERLAY}var d=document.documentElement,b=document.body;\
         return JSON.stringify({{width:Math.max(d.scrollWidth,b?b.scrollWidth:0)||innerWidth,\
         height:Math.max(d.scrollHeight,b?b.scrollHeight:0)||innerHeight}});}})()"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_text_is_embedded_as_escaped_string_literal() {
        let js = type_text(2, "a\"b'c</script>\n\u{2028}", true);
        assert!(js.contains(r#"var text="a\"b'c</script>\n\u2028";"#), "{js}");
        assert!(js.contains("if(true)"));
        let js = run_js("document.title = \"x\"");
        assert!(js.contains(r#"(0,eval)("document.title = \"x\"")"#), "{js}");
        assert!(js.contains("await r"));
        assert!(js.starts_with("if(Array.prototype.some.call(document.querySelectorAll('input[type=password]')"), "pengaman password dulu");
    }

    #[test]
    fn element_numbers_are_numeric_only() {
        assert!(click(7).contains(r#"querySelector('[data-mb-ref="7"]')"#), "{}", click(7));
    }

    #[test]
    fn youtube_seek_without_value_is_an_error() {
        assert!(youtube(YoutubeAction::Seek, None).contains("error"));
        assert!(youtube(YoutubeAction::Seek, Some(f64::NAN)).contains("error"));
        assert!(youtube(YoutubeAction::Seek, Some(90.0)).contains("p.seekTo(90,true);"));
    }

    #[test]
    fn developer_scripts_validate_their_inputs() {
        assert!(wait_for(None, None, WaitState::Visible, 1000).contains("butuh selector atau text"));
        assert!(wait_for(Some("#a"), None, WaitState::Visible, 999_999).contains("timeout=30000"));
        assert!(wait_for(Some("a'b"), Some("x"), WaitState::Hidden, 10).contains(r#"var sel="a'b",txt="x""#));
        assert!(inspect_element(None, None, None).contains("butuh selector atau element"));
        assert!(inspect_element(None, Some(3), None).contains(r#"var q="[data-mb-ref=\"3\"]""#));
        let js = inspect_element(Some("#x"), None, Some(&["color".into(), "x;alert(1)".into()]));
        assert!(js.contains(r#"["color"].forEach"#), "properti CSS yang aneh dibuang: {js}");
        assert!(console_logs(LogLevel::Error, 5, 20).contains(">=2&&e.seq>5"));
        assert!(network_log(true, Some("api/'x"), false, 10).contains(r#"var c="api/'x""#));
    }

    #[test]
    fn selection_scripts_share_inspect_and_escape_selectors() {
        let picked = picked_element(None);
        assert!(picked.contains("window.__mbPicked") && picked.contains("function inspect(e)"));
        assert!(picked.contains("Cmd+Shift+E"));
        for script in [picked, element_box("#a"), document_size()] {
            assert!(script.starts_with("(function(){if(!window.__mbPicker)"), "tanda Terpilih dibuang dulu: {script}");
        }
        let boxed = element_box("a'b\"c");
        assert!(boxed.contains(r#"var q="a'b\"c""#), "{boxed}");
        assert!(inspect_element(Some("#x"), None, None).contains("o.matches=count"));
    }

    #[test]
    fn read_page_limits_text_length() {
        assert!(read_page(500).contains("t.slice(0,500)"));
    }
}
