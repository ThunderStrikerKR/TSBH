//! Measured text fit, display-only compact markers and bounded caches.
use eframe::egui::{self,Ui};
use crate::typography;

// Preserve the exact existing prefix/ellipsis measurement and reuse it
// across idle frames. The full galley is still supplied by egui.
thread_local!{
    static CLIENT_NAME_FIT:std::cell::RefCell<Vec<(String,u32,u32,std::sync::Arc<str>)>>=
        const{std::cell::RefCell::new(Vec::new())};
}

pub(crate) fn fitted_client_name(ui:&Ui,name:&str,limit:f32)->std::sync::Arc<str>{
    let scale=ui.ctx().pixels_per_point().to_bits();
    CLIENT_NAME_FIT.with(|cache|{
        let mut cache=cache.borrow_mut();
        if let Some(index)=cache.iter().position(|(text,width,dpi,_)|
            text==name && *width==limit.to_bits() && *dpi==scale){
            let entry=cache.remove(index);let shown=entry.3.clone();cache.push(entry);return shown;
        }
        let measure=|text:&str|typography::management_table_cell_galley(
            ui,text,typography::ManagementTableCellRole::ClientName).size().x;
        let mut shown=String::new();
        if measure("…")<=limit{
            for ch in name.chars(){
                let next=format!("{shown}{ch}…");
                if measure(&next)>limit{break;}
                shown.push(ch);
            }
            shown.push('…');
        }
        let shown:std::sync::Arc<str>=shown.into();
        if cache.len()>=64{cache.remove(0);}
        cache.push((name.to_owned(),limit.to_bits(),scale,shown.clone()));shown
    })
}

/// Ellipsize only the displayed product label, using the exact 16pt glyph
/// width. This does not alter the Rust item title, matching key or hover text.
/// Unicode char boundaries prevent cutting a Hangul syllable or UTF-8 byte.
// Cache the exact measured truncation, not the galley: the latter still
// uses egui's current font atlas and active highlight ink.
#[derive(Clone)]
struct ReceptionTextFit{ text:String,width:u32,scale:u32,font:egui::FontId,shown:std::sync::Arc<str> }

thread_local!{
    static RECEPTION_TEXT_FIT:std::cell::RefCell<Vec<ReceptionTextFit>>=
        const{std::cell::RefCell::new(Vec::new())};
}

pub(crate) fn cached_reception_product(ui:&Ui,text:&str,width:f32,font:&egui::FontId,color:egui::Color32)->std::sync::Arc<str>{
    let scale=ui.ctx().pixels_per_point().to_bits();
    RECEPTION_TEXT_FIT.with(|cache|{
        let mut cache=cache.borrow_mut();
        if let Some(index)=cache.iter().position(|entry|entry.text==text &&
            entry.width==width.to_bits() && entry.scale==scale && &entry.font==font){
            let entry=cache.remove(index);let shown=entry.shown.clone();cache.push(entry);return shown;
        }
        let shown:std::sync::Arc<str>=reception_ellipsized_product(text,width,|candidate|
            ui.painter().layout_no_wrap(candidate.to_owned(),font.clone(),color).size().x).into();
        if cache.len()>=128{cache.remove(0);}
        cache.push(ReceptionTextFit{text:text.to_owned(),width:width.to_bits(),scale,font:font.clone(),shown:shown.clone()});
        shown
    })
}

pub fn clear_reception_text_fit(){
    RECEPTION_TEXT_FIT.with(|cache|cache.borrow_mut().clear());
    CLIENT_NAME_FIT.with(|cache|cache.borrow_mut().clear());
}

pub(crate) fn reception_ellipsized_product(
    full:&str,max_width:f32,mut measure:impl FnMut(&str)->f32
)->String {
    if max_width<=0.0 {return String::new();}
    if measure(full)<=max_width {return full.to_owned();}
    if measure("…")>max_width {return String::new();}
    let boundaries:Vec<usize>=full.char_indices().map(|(i,_)|i)
        .chain(std::iter::once(full.len())).collect();
    let mut low=0usize;
    let mut high=boundaries.len()-1;
    while low<high {
        let mid=low+(high-low+1)/2;
        let candidate=format!("{}…",&full[..boundaries[mid]]);
        if measure(&candidate)<=max_width {low=mid;}
        else {high=mid-1;}
    }
    format!("{}…",full[..boundaries[low]].trim_end())
}

/// A deliberately compact presentation for exceptional quantities.
/// Backend quantities, pricing and full-value tooltips remain untouched.
pub(crate) fn reception_compact_quantity(full:&str)->&str {
    let count=full.strip_suffix('개')
        .and_then(|digits|digits.parse::<u64>().ok());
    if count.is_some_and(|n|n>=1000) {"999+"} else {full}
}

pub(crate) fn reception_pending_money_marker(value:&str)->bool{
    matches!(value.trim(), "-" | "–" | "—")
}
