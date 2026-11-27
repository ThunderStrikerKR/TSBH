//! Paint and one-shot reveal parity with the archived React one-touch hover contract.
use eframe::egui::{self,Color32,Context,Rect,Stroke,Ui};
use crate::{theme,typography};
pub const INK:Color32=theme::ONE_TOUCH_TEXT;
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
/// Shared read-only value emphasis: pad the measured text bounds on all
/// sides, with the allocated control slot (not the text clip) as the limit.
pub fn value_rect(ink:Rect,slot:Rect)->Rect{
    ink.expand(theme::SPACE_4).intersect(slot)
}
fn background_shape(rect:Rect)->egui::Shape{
    egui::Shape::rect_filled(rect,theme::CONTROL_RADIUS,theme::ONE_TOUCH_BG)
}
fn outline_shape(rect:Rect)->egui::Shape{
    egui::Shape::rect_stroke(rect,theme::CONTROL_RADIUS,
        Stroke::new(theme::ONE_TOUCH_BORDER_WIDTH,theme::ONE_TOUCH_BORDER),egui::StrokeKind::Inside)
}
pub fn background(ui:&Ui,rect:Rect){
    if let Some(painter)=crate::ui_primitives::highlight_painter(ui,rect){
        painter.add(background_shape(rect));
    }
}
pub fn outline(ui:&Ui,rect:Rect){
    if let Some(painter)=crate::ui_primitives::highlight_painter(ui,rect){
        painter.add(outline_shape(rect));
    }
}
/// Existing widgets with their own fill: tint lightly without covering text.
pub fn mark(ui:&Ui,rect:Rect){
    if let Some(painter)=crate::ui_primitives::highlight_painter(ui,rect){
        painter.rect_filled(rect,theme::CONTROL_RADIUS,theme::ONE_TOUCH_TINT);
    }
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
/// Both states use the shared Reception document typography and fixed line pitch.
fn payment_job(text:&str,signature:&str,emphasis:bool,width:f32)->egui::text::LayoutJob{
    let mut job=egui::text::LayoutJob::default();
    job.wrap.max_width=width;
    job.first_row_min_height=typography::RECEPTION_SOURCE_LINE_HEIGHT;
    let format=|active|egui::TextFormat{
        font_id:egui::FontId::proportional(typography::FONT_RECEPTION_SOURCE_TEXT),
        line_height:Some(typography::RECEPTION_SOURCE_LINE_HEIGHT),
        color:if active{INK}else{theme::TEXT},
        ..Default::default()};
    let mut start=0;
    for (a,b) in payment_ranges(text,signature){
        if a<start{continue;}
        job.append(&text[start..a],0.0,format(false));
        job.append(&text[a..b],0.0,format(emphasis));
        start=b;
    }
    job.append(&text[start..],0.0,format(false));
    job
}
fn payment_rects(text:&str,signature:&str,galley:&egui::Galley,pos:egui::Pos2)->Vec<Rect>{
    let ranges=payment_ranges(text,signature).into_iter().map(|(a,b)|
        (text[..a].chars().count(),text[..b].chars().count())).collect::<Vec<_>>();
    let chars=text.chars().collect::<Vec<_>>();
    let mut row_start=0;
    let mut rects=Vec::new();
    for (index,placed) in galley.rows.iter().enumerate(){
        let glyphs=&placed.row.glyphs;
        let row_end=row_start+glyphs.len();
        for &(a,b) in &ranges{
            let start=a.max(row_start);
            let end=b.min(row_end);
            if start>=end{continue;}
            let first=&glyphs[start-row_start];
            let last=&glyphs[end-row_start-1];
            let x=pos.x+placed.pos.x;
            let y=pos.y+placed.pos.y;
            let left=x+first.pos.x;
            let right=x+last.pos.x+last.advance_width;
            if right>left{
                rects.push(Rect::from_min_max(egui::pos2(left-2.0,y),
                    egui::pos2(right+2.0,y+placed.row.size.y)));
            }
        }
        row_start=row_end;
        if index+1<galley.rows.len() && chars.get(row_start)==Some(&'\n'){row_start+=1;}
    }
    rects
}
pub fn payment_message(ui:&mut Ui,text:&str,signature:&str,emphasis:bool){
    let mut immutable=text;
    let mut layouter=|layout_ui:&egui::Ui,buffer:&dyn egui::TextBuffer,width:f32|{
        layout_ui.fonts_mut(|fonts|fonts.layout_job(
            payment_job(buffer.as_str(),signature,emphasis,width)))
    };
    let paint_slot=ui.painter().add(egui::Shape::Noop);
    let display=crate::input::reception_source_text_edit(&mut immutable,ui.available_width())
        .layouter(&mut layouter).show(ui);
    if emphasis{
        // Text ink clipping is narrower than the reserved document surface.
        let clip=ui.clip_rect().intersect(display.response.rect);
        let painter=ui.painter().with_clip_rect(clip);
        let rects=payment_rects(text,signature,&display.galley,display.galley_pos);
        painter.set(paint_slot,egui::Shape::Vec(rects.iter().copied()
            .filter(|rect|rect.intersects(clip))
            .map(background_shape)
            .collect()));
        for rect in rects.into_iter().filter(|rect|rect.intersects(clip)){
            painter.add(outline_shape(rect));
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn value_emphasis_reserves_equal_padding_inside_control_slot(){
        let slot=Rect::from_min_size(egui::pos2(0.0,0.0),egui::vec2(120.0,42.0));
        let ink=Rect::from_min_size(egui::pos2(25.0,18.0),egui::vec2(91.0,16.0));
        let frame=value_rect(ink,slot);
        assert_eq!(frame,ink.expand(theme::SPACE_4));
        assert_eq!(frame.center(),ink.center());
        assert_eq!(frame.right(),slot.right());
    }
    #[test] fn one_touch_palette_keeps_orange_fill_and_tint(){
        assert_eq!(theme::ONE_TOUCH_BG,Color32::from_rgb(59,43,27));
        assert_ne!(theme::ONE_TOUCH_BG,theme::PANEL);
        assert_ne!(theme::ONE_TOUCH_BG,theme::INPUT);
        assert_eq!(theme::ONE_TOUCH_TEXT,Color32::from_rgb(255,228,188));
        assert_eq!(theme::ONE_TOUCH_BORDER,Color32::from_rgb(245,158,11));
        assert_eq!(theme::ONE_TOUCH_BORDER_WIDTH,2.0);
        assert!(theme::ONE_TOUCH_TINT.a()>0 && theme::ONE_TOUCH_TINT.a()<255);
        let ctx=egui::Context::default();
        let output=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let (r,_)=ui.allocate_exact_size(egui::vec2(100.0,30.0),egui::Sense::hover());
                background(ui,r);outline(ui,r);
                let (r,_)=ui.allocate_exact_size(egui::vec2(100.0,30.0),egui::Sense::hover());
                mark(ui,r);
            });
        });
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(r) if r.fill==theme::ONE_TOUCH_BG)));
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(r) if r.fill==theme::ONE_TOUCH_TINT)));
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(r) if r.stroke.color==theme::ONE_TOUCH_BORDER && r.stroke.width==theme::ONE_TOUCH_BORDER_WIDTH)));
        let job=payment_job("합계 8,000원 (부가세 포함)","",true,400.0);
        assert!(job.sections.iter().any(|s|s.format.color==INK));
        assert!(job.sections.iter().all(|s|s.format.background==Color32::TRANSPARENT));
    }
    #[test] fn payment_message_hover_preserves_document_metrics_and_uses_shared_outline(){
        let text="상품 7,000원\n\n합계 8,000원 (부가세 포함)\n은행 123 예금주";
        let signature=r#"{"account":{"bank":"은행","number":"123","holder":"예금주"}}"#;
        let ctx=egui::Context::default();
        typography::install(&ctx);
        theme::install(&ctx);
        let output=ctx.run(egui::RawInput::default(),|ctx|{
            egui::CentralPanel::default().show(ctx,|ui|{
                let normal=payment_job(text,signature,false,170.0);
                let hover=payment_job(text,signature,true,170.0);
                assert_eq!(normal.sections.len(),hover.sections.len());
                for (a,b) in normal.sections.iter().zip(&hover.sections){
                    assert_eq!(a.byte_range,b.byte_range);
                    assert_eq!(a.format.font_id,b.format.font_id);
                    assert_eq!(a.format.font_id.size,typography::FONT_RECEPTION_SOURCE_TEXT);
                    assert_eq!(a.format.line_height,Some(typography::RECEPTION_SOURCE_LINE_HEIGHT));
                    assert_eq!(a.format.line_height,b.format.line_height);
                }
                let (a,b)=ui.fonts_mut(|f|(f.layout_job(normal),f.layout_job(hover)));
                assert_eq!(a.size(),b.size());
                assert_eq!(a.rows.len(),b.rows.len());
                for (a,b) in a.rows.iter().zip(&b.rows){
                    assert_eq!(a.pos,b.pos);
                    assert_eq!(a.row.size,b.row.size);
                }
                payment_message(ui,text,signature,true);
            });
        });
        assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r)
            if r.stroke.color==theme::ONE_TOUCH_BORDER
                && r.stroke.width==theme::ONE_TOUCH_BORDER_WIDTH)));
    }
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
