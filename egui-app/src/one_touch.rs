//! Paint and one-shot reveal parity with the archived React one-touch hover contract.
use eframe::egui::{self,Color32,Context,Rect,Stroke,Ui};
use crate::{theme,typography};
pub const INK:Color32=Color32::from_rgb(255,228,188);
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum Stage{#[default] None,Review,Freight,PaymentCopy,PaymentConfirm}
impl Stage{
    pub fn from_step(step:usize)->Self{match step{0=>Self::Review,1=>Self::Freight,2=>Self::PaymentCopy,_=>Self::PaymentConfirm}}
    pub fn recipient(self,index:usize,remote:bool)->bool{
        self==Self::Review || (self==Self::Freight && index==1 && remote)
    }
    pub fn item(self,index:usize)->bool{self==Self::Review || (self==Self::Freight && index<2)}
    pub fn freight(self)->bool{matches!(self,Self::Review|Self::Freight)}
}
#[derive(Clone,Copy,Default)]
struct Scope{stage:Stage,remote:bool}
fn scope_id()->egui::Id{egui::Id::new("reception-one-touch-paint")}
pub fn set(ctx:&Context,stage:Stage,remote:bool){
    ctx.data_mut(|d|d.insert_temp(scope_id(),Scope{stage,remote}));
}
fn get(ctx:&Context)->Scope{ctx.data(|d|d.get_temp::<Scope>(scope_id()).unwrap_or_default())}
pub fn stage(ui:&Ui)->Stage{get(ui.ctx()).stage}
pub fn recipient(ui:&Ui,index:usize)->bool{let s=get(ui.ctx());s.stage.recipient(index,s.remote)}
/// A hover reveal belongs to a single stage and exact displayed shipment/draft
/// recipient. Repeated frames and the employee's manual scrolling must not
/// restart it. Leaving the preview clears the latch.
#[derive(Clone,Default)]
struct Reveal {
    key:Option<(Stage,String,String)>,
    pending:bool,
}
impl Reveal {
    fn prepare(&mut self,stage:Stage,owner:&str,target:&str){
        let key=matches!(stage,Stage::Review|Stage::Freight)
            .then(||(stage,owner.to_owned(),target.to_owned()));
        if self.key!=key{
            self.pending=key.is_some();
            self.key=key;
        }
    }
    fn take(&mut self,stage:Stage)->bool{
        if self.pending && self.key.as_ref().is_some_and(|key|key.0==stage){
            self.pending=false;
            true
        }else{false}
    }
}
fn reveal_id()->egui::Id{egui::Id::new("reception-one-touch-reveal")}
pub fn prepare_reveal(ctx:&Context,stage:Stage,owner:&str,target:&str){
    ctx.data_mut(|d|{
        let mut reveal=d.get_temp::<Reveal>(reveal_id()).unwrap_or_default();
        reveal.prepare(stage,owner,target);
        d.insert_temp(reveal_id(),reveal);
    });
}
/// Called inside the actual target's scroll area, not the fixed pane.
/// None uses nearest alignment. Never request keyboard focus or selection.
pub fn reveal_target(ui:&Ui,expected:Stage,rect:Rect){
    if stage(ui)!=expected || !rect.is_positive() || !ui.clip_rect().is_positive(){return;}
    let reveal=ui.ctx().data_mut(|d|{
        let mut state=d.get_temp::<Reveal>(reveal_id()).unwrap_or_default();
        let requested=state.take(expected);
        d.insert_temp(reveal_id(),state);
        requested
    });
    if reveal && !ui.clip_rect().contains_rect(rect){
        ui.scroll_to_rect(rect,None);
    }
}
pub fn background(ui:&Ui,rect:Rect){
    ui.painter().rect_filled(rect,theme::CONTROL_RADIUS,theme::ORANGE_EMPHASIS_BG);
}
pub fn outline(ui:&Ui,rect:Rect){
    if rect.is_positive(){ui.painter().with_clip_rect(ui.clip_rect().intersect(rect))
        .rect_stroke(rect,theme::CONTROL_RADIUS,Stroke::new(2.0,theme::SELECTED_ORDER),egui::StrokeKind::Inside);}
}
/// Existing widgets with their own fill: tint lightly without covering text.
pub fn mark(ui:&Ui,rect:Rect){
    ui.painter().rect_filled(rect,theme::CONTROL_RADIUS,Color32::from_rgba_unmultiplied(245,158,11,18));
    outline(ui,rect);
}
/// Byte ranges of the exact printed total, tax label, and native account.
/// Never infer a bank or calculate a monetary value.
pub fn payment_ranges(text:&str,signature:&str)->Vec<(usize,usize)>{
    let mut ranges=Vec::new();
    let vat=["(부가세 포함/별도)","(부가세 포함)","(부가세 별도)","(부가세 일부 포함)"]
        .iter().filter_map(|v|text.rfind(*v).map(|start|(start,start+v.len()))).max_by_key(|r|r.0);
    if let Some((start,end))=vat{
        ranges.push((start,end));
        let line_start=text[..start].rfind('\n').map_or(0,|i|i+1);
        let line=&text[line_start..start];
        static AMOUNT:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
        let pattern=AMOUNT.get_or_init(||regex::Regex::new(r"[0-9][0-9,]*원")
            .expect("static payment amount pattern"));
        if let Some(amount)=pattern.find_iter(line).last(){
            ranges.push((line_start+amount.start(),line_start+amount.end()));
        }
    }
    if let Ok(value)=serde_json::from_str::<serde_json::Value>(signature){
        let account=&value["account"];
        let parts=["bank","number","holder"].map(|key|account[key].as_str().unwrap_or("").trim());
        if parts.iter().all(|p|!p.is_empty()){
            let mut offset=vat.map_or(0,|(_,end)|end);
            for line in text[offset..].split_inclusive('\n'){
                let found=parts.map(|p|line.find(p).map(|i|(i,i+p.len())));
                if let [Some(a),Some(b),Some(c)]=found{
                    ranges.push((offset+a.0.min(b.0).min(c.0),offset+a.1.max(b.1).max(c.1)));
                    break;
                }
                offset+=line.len();
            }
        }
    }
    ranges.sort_unstable();
    ranges
}
pub fn payment_job(text:&str,signature:&str)->egui::text::LayoutJob{
    let mut job=egui::text::LayoutJob::default();
    let format=|active|egui::TextFormat{font_id:egui::FontId::proportional(typography::FONT_BODY),
        color:if active{INK}else{theme::TEXT},
        background:if active{theme::ORANGE_EMPHASIS_BG}else{Color32::TRANSPARENT},
        ..Default::default()};
    let mut start=0;
    for (a,b) in payment_ranges(text,signature){
        if a<start{continue;}
        job.append(&text[start..a],0.0,format(false));
        job.append(&text[a..b],0.0,format(true));
        start=b;
    }
    job.append(&text[start..],0.0,format(false));
    job
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn freight_scope_excludes_price_and_non_remote_address(){
        assert!(Stage::Freight.item(0)&&Stage::Freight.item(1));
        assert!(!Stage::Freight.item(2)&&!Stage::Freight.recipient(0,true));
        assert!(Stage::Freight.recipient(1,true)&&!Stage::Freight.recipient(1,false));
        assert!(!Stage::PaymentCopy.freight()&&!Stage::PaymentConfirm.item(0));
    }
    #[test] fn reveal_is_once_per_stage_and_target_and_rearms_after_exit(){
        let mut state=Reveal::default();
        state.prepare(Stage::Review,"draft","recipient-a");
        assert!(!state.take(Stage::Freight));
        assert!(state.take(Stage::Review));
        state.prepare(Stage::Review,"draft","recipient-a");
        assert!(!state.take(Stage::Review));
        state.prepare(Stage::Review,"draft","recipient-b");
        assert!(state.take(Stage::Review));
        state.prepare(Stage::Freight,"group","shipment-a");
        assert!(state.take(Stage::Freight));
        state.prepare(Stage::None,"","");
        state.prepare(Stage::Freight,"group","shipment-a");
        assert!(state.take(Stage::Freight));
        state.prepare(Stage::PaymentCopy,"group","shipment-a");
        assert!(!state.take(Stage::PaymentCopy));
    }
    #[test] fn payment_amount_ignores_later_non_currency_won(){
        let text="상품 7,000원\n합계 8,000원 입금 원하시면 (부가세 일부 포함)";
        let ranges=payment_ranges(text,"");
        assert_eq!(ranges.iter().map(|&(a,b)|&text[a..b]).collect::<Vec<_>>(),
            vec!["8,000원","(부가세 일부 포함)"]);
    }
    #[test] fn exact_payment_ranges_preserve_original_text(){
        let text="상품 7,000원\n합계 8,000원 입니다. (부가세 포함)\n은행 1234 예금주 입니다.";
        let signature=r#"{"account":{"bank":"은행","number":"1234","holder":"예금주"}}"#;
        let ranges=payment_ranges(text,signature);
        let slices=ranges.iter().map(|&(a,b)|&text[a..b]).collect::<Vec<_>>();
        assert_eq!(slices,vec!["8,000원","(부가세 포함)","은행 1234 예금주"]);
    }
}
