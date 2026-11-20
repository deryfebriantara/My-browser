//! Script halaman untuk kontrol dari Claude. Setiap script menghasilkan satu
//! string JSON, berisi data atau `{"error": "..."}`.
//!
//! Elemen yang bisa diklik atau diisi diberi nomor oleh `list_elements`
//! (atribut `data-mb-ref`); `click` dan `type_text` memakai nomor itu.

use crate::control::{ScrollDirection, YoutubeAction};

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

pub fn run_js(code: &str) -> String {
    format!(
        "(function(){{var r;try{{r=(0,eval)({code});}}catch(e){{return JSON.stringify({{error:String(e)}});}}\
         if(r&&typeof r.then==='function')return JSON.stringify({{result:'[Promise] hasil async tidak ditunggu'}});\
         try{{return JSON.stringify({{result:r===undefined?null:r}});}}catch(e){{return JSON.stringify({{result:String(r)}});}}}})()",
        code = js_string(code)
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
    fn read_page_limits_text_length() {
        assert!(read_page(500).contains("t.slice(0,500)"));
    }
}
