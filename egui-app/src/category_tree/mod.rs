//! Shared category tree geometry, rendering and interaction surface.
use crate::ui_layout::geometry::UiWidthPolicyExt as _;
use crate::ui_layout::geometry::SharedPainterClipExt as _;
use crate::ui_layout::geometry::SharedAllocationExt as _;
use eframe::egui::{self,Color32,Rect,Response,Stroke,Ui,Vec2};
use crate::{theme,typography};
use crate::ui_primitives::{overflow_tooltip,paint_management_chevron,ManagementChevronDirection};

// Private tree decoration and slot geometry.
const CATALOG_TREE_INSET:f32 = theme::SPACE_12;
const CATALOG_TREE_CHEVRON_SIZE:f32 = theme::SPACE_16;
const CATALOG_TREE_FOLDER_SIZE:f32 = 20.0;
const CATALOG_TREE_GAP:f32 = theme::SPACE_8;
const CATALOG_TREE_TERMINAL_GAP:f32 = theme::SPACE_4;
const CATALOG_TREE_GUIDE_COLOR:Color32 = Color32::from_rgb(74,74,74);
const CATALOG_TREE_GUIDE_WIDTH:f32 = 1.0;
const CATALOG_TREE_FOLDER_STROKE:f32 = 1.5;
const CATALOG_TREE_FOLDER_COLOR:Color32 = theme::text_ink(theme::TextRole::Label,theme::TextState::Normal);

/// CategoryTree presentation contract. The caller owns data, selection and
/// expansion; all tree row geometry and painting are centralized here.
pub(crate) struct CategoryTreeRow<'a>{
    pub(crate) name:&'a str,
    pub(crate) summary:&'a str,
    pub(crate) depth:usize,
    /// For each ancestor axis above the immediate parent, whether a later
    /// sibling still needs that vertical rail (root axis is index zero).
    pub(crate) ancestor_guides:&'a [bool],
    pub(crate) is_last:bool,
    pub(crate) root:bool,
    pub(crate) has_children:bool,
    pub(crate) open:bool,
    pub(crate) selected:bool,
}

struct CategoryTreeGeometry{
    row:Rect,
    chevron:Rect,
    folder:Rect,
    name_x:f32,
    depth:usize,
}

impl CategoryTreeGeometry{
    fn new(row:Rect,depth:usize)->Self{
        let chevron=Rect::from_min_size(
            egui::pos2(row.left()+CATALOG_TREE_INSET
                +depth as f32*theme::CATALOG_TREE_INDENT,
                row.center().y-CATALOG_TREE_CHEVRON_SIZE/2.0),
            Vec2::splat(CATALOG_TREE_CHEVRON_SIZE)
        );
        let folder=Rect::from_min_size(
            egui::pos2(chevron.right()+CATALOG_TREE_GAP,
                row.center().y-CATALOG_TREE_FOLDER_SIZE/2.0),
            Vec2::splat(CATALOG_TREE_FOLDER_SIZE)
        );
        Self{row,chevron,folder,name_x:folder.right()+CATALOG_TREE_GAP,depth}
    }
    fn axis(&self,level:usize)->f32{
        self.row.left()+CATALOG_TREE_INSET
            +CATALOG_TREE_CHEVRON_SIZE/2.0
            +level as f32*theme::CATALOG_TREE_INDENT
    }
    fn guides(&self,ancestor_guides:&[bool],is_last:bool,has_children:bool,
        open:bool,mut draw:impl FnMut([egui::Pos2;2])){
        let y=self.row.center().y;
        let top=self.row.top();
        let bottom=self.row.bottom();
        let p=egui::pos2;
        // Older ancestor rails never overlap the immediate parent elbow.
        for (level,&continued) in ancestor_guides.iter().take(self.depth.saturating_sub(1)).enumerate(){
            if continued{
                let x=self.axis(level);
                draw([p(x,top),p(x,bottom)]);
            }
        }
        if self.depth>0{
            let x=self.axis(self.depth-1);
            draw([p(x,top),p(x,if is_last{y}else{bottom})]);
            // Leaves reserve the same chevron column, but the elbow extends
            // through that empty slot to the folder instead of stopping short.
            let target=if has_children{self.chevron.left()}else{self.folder.left()};
            draw([p(x,y),p((target-CATALOG_TREE_TERMINAL_GAP).max(x),y)]);
        }
        if has_children&&open{
            let x=self.axis(self.depth);
            draw([p(x,(self.chevron.bottom()+CATALOG_TREE_TERMINAL_GAP).min(bottom)),p(x,bottom)]);
        }
    }
}

/// Font-independent 20pt vector folder, with a distinct open front flap.
fn paint_category_tree_folder(painter:&egui::Painter,rect:Rect,open:bool,ink:Color32){
    let p=|x:f32,y:f32|egui::pos2(rect.left()+rect.width()*x/20.0,
        rect.top()+rect.height()*y/20.0);
    let stroke=Stroke::new(CATALOG_TREE_FOLDER_STROKE,ink);
    if open{
        painter.add(egui::Shape::line(vec![
            p(2.0,16.5),p(2.0,4.0),p(7.0,4.0),p(9.0,6.0),p(16.0,6.0),p(16.0,8.0)
        ],stroke));
        painter.add(egui::Shape::closed_line(vec![
            p(2.0,16.5),p(5.0,8.5),p(18.5,8.5),p(15.5,16.5)
        ],stroke));
    }else{
        painter.add(egui::Shape::closed_line(vec![
            p(2.0,16.5),p(2.0,4.0),p(7.0,4.0),p(9.0,6.0),
            p(18.0,6.0),p(18.0,16.5)
        ],stroke));
    }
}

pub(crate) fn category_tree_row(ui:&mut Ui,item:CategoryTreeRow<'_>)->Response{
    tree_row(ui,item,false,true,false)
}

/// Management-only folder row. Ordinary category pickers remain click-only.
pub(crate) fn draggable_folder_row(ui:&mut Ui,item:CategoryTreeRow<'_>,id:&str,parent:&str,enabled:bool,draggable:bool)->Response{
    let name=item.name;
    let response=tree_row(ui,item,false,enabled,draggable);
    crate::drag_drop::start_native(ui,&response,enabled&&draggable&&!id.is_empty(),||
        FolderDrag{id:id.to_owned(),parent:parent.to_owned(),name:name.to_owned()});
    response
}

#[derive(Clone)]
struct FolderDrag{id:String,parent:String,name:String}

fn valid_folder_target(drag:&FolderDrag,target:&str,contains_source:impl Fn(&str)->bool)->bool{
    !target.is_empty()&&drag.id!=target&&drag.parent!=target&&!contains_source(&drag.id)
}

#[derive(Clone,Debug)]
pub(crate) struct FolderMove{
    pub(crate) id:String,
    pub(crate) parent:String,
    pub(crate) target:String,
}

pub(crate) fn folder_drop_target(ui:&Ui,response:&Response,target:&str,enabled:bool,
    contains_source:impl Fn(&str)->bool)->Option<FolderMove>{
    if !enabled||!ui.is_enabled()||target.is_empty(){return None;}
    let valid=|drag:&FolderDrag|valid_folder_target(drag,target,&contains_source);
    crate::drag_drop::drop_native::<FolderDrag>(ui,response,enabled,valid)
        .map(|drag|FolderMove{id:drag.id.clone(),parent:drag.parent.clone(),target:target.to_owned()})
}

/// Product leaf opens the existing product detail; it never changes
/// category selection or expansion.
pub(crate) fn product_tree_row(ui:&mut Ui,name:&str,depth:usize,ancestor_guides:&[bool],is_last:bool,enabled:bool,draggable:bool,selected:bool)->Response{
    tree_row(ui,CategoryTreeRow{
        name,summary:"",depth,ancestor_guides,is_last,
        root:false,has_children:false,open:false,selected,
    },true,enabled,draggable)
}

#[derive(Clone)]
struct ProductDrag { id:String, source:String, name:String, members:Vec<(String,String)> }

/// Selection is shared between both management tree modes. A plain click
/// still opens details; Ctrl+click toggles a unique product ID.
#[derive(Default)]
pub(crate) struct ProductSelection(std::collections::BTreeMap<String,String>);
impl ProductSelection {
    pub(crate) fn contains(&self,id:&str)->bool{self.0.contains_key(id)}
    pub(crate) fn clear(&mut self){self.0.clear();}
    pub(crate) fn toggle(&mut self,id:&str,source:&str){
        if self.0.remove(id).is_none(){self.0.insert(id.to_owned(),source.to_owned());}
    }
    fn members(&self,id:&str,source:&str)->Vec<(String,String)>{
        if self.contains(id){self.0.iter().map(|(id,source)|(id.clone(),source.clone())).collect()}
        else{vec![(id.to_owned(),source.to_owned())]}
    }
}

const CATEGORY_DRAG_OPEN_DELAY:f64=0.65;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum DragKind{Product,Folder}

/// One continuous hover per drag kind/source/target, scoped to a tree view.
#[derive(Clone,Debug,Default)]
pub(crate) struct CategoryDragHover {
    pending:Option<(DragKind,String,String,String,f64)>,
    seen:bool,
}
impl CategoryDragHover {
    pub(crate) fn begin_frame(&mut self){self.seen=false;}
    pub(crate) fn end_frame(&mut self){
        if !self.seen{self.pending=None;}
    }
    fn advance(&mut self,kind:DragKind,id:&str,source:&str,target:&str,now:f64)->bool{
        self.seen=true;
        if !self.pending.as_ref().is_some_and(|(k,p,s,t,_)|
            *k==kind&&p==id&&s==source&&t==target){
            self.pending=Some((kind,id.to_owned(),source.to_owned(),target.to_owned(),now));
        }
        let elapsed=self.pending.as_ref().map(|(_,_,_,_,start)|now-*start).unwrap_or(0.0);
        if elapsed>=CATEGORY_DRAG_OPEN_DELAY{
            self.pending=None;
            true
        }else{false}
    }
    pub(crate) fn observe(&mut self,ui:&Ui,response:&Response,target:&str,enabled:bool,
        folder_enabled:bool,contains_source:impl Fn(&str)->bool)->bool{
        if !enabled||!ui.is_enabled()||target.is_empty()||
            !ui.input(|i|i.pointer.primary_down())||
            !response.rect.intersect(ui.clip_rect()).is_positive(){return false;}
        let now=ui.input(|i|i.time);
        let ready=if let Some(drag)=crate::drag_drop::hover_payload::<ProductDrag>(response){
            if drag.members.iter().all(|(_,source)|source==target){return false;}
            self.advance(DragKind::Product,&drag.id,&drag.source,target,now)
        }else if let Some(drag)=crate::drag_drop::hover_payload::<FolderDrag>(response){
            if !folder_enabled||!valid_folder_target(&drag,target,contains_source){return false;}
            self.advance(DragKind::Folder,&drag.id,&drag.parent,target,now)
        }else{return false;};
        if ready{return true;}
        if let Some((_,_,_,_,start))=&self.pending{
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(
                (CATEGORY_DRAG_OPEN_DELAY-(now-*start)).max(0.0)));
        }
        false
    }
}

pub(crate) struct ProductMove {
    pub(crate) id:String,
    pub(crate) source:String,
    pub(crate) target:String,
}

pub(crate) fn start_product_drag(ui:&Ui,response:&Response,id:&str,name:&str,source:&str,enabled:bool,selection:&ProductSelection){
    crate::drag_drop::start_native(ui,response,enabled&&!id.is_empty()&&!source.is_empty(),||
        ProductDrag{id:id.to_owned(),source:source.to_owned(),name:name.to_owned(),members:selection.members(id,source)});
}

/// Read the native payload, not the source row: focused navigation may
/// remove that row while the mouse remains pressed.
pub(crate) fn paint_drag_preview(ui:&Ui){
    if let Some(drag)=crate::drag_drop::native_payload::<ProductDrag>(ui.ctx()){
        crate::drag_drop::paint_pointer_label(ui,&drag.name,&format!("상품 {}개",drag.members.len()));
    }else if let Some(drag)=crate::drag_drop::native_payload::<FolderDrag>(ui.ctx()){
        crate::drag_drop::paint_pointer_label(ui,&drag.name,"폴더 1개");
    }
}

/// Native typed drag/drop; only real folders are accepted. The source
/// remains untouched until ProductManager commits a Rust mutation.
pub(crate) fn product_drop_target(ui:&Ui,response:&Response,target:&str,enabled:bool)->Option<Vec<ProductMove>>{
    if !enabled||!ui.is_enabled()||target.is_empty(){return None;}
    crate::drag_drop::drop_native::<ProductDrag>(ui,response,enabled,
        |drag|drag.members.iter().any(|(_,source)|source!=target)).and_then(|drag|{
        let moves=drag.members.iter().filter(|(_,source)|source!=target)
            .map(|(id,source)|ProductMove{id:id.clone(),source:source.clone(),target:target.to_owned()})
            .collect::<Vec<_>>();
        (!moves.is_empty()).then_some(moves)
    })
}

fn tree_row(ui:&mut Ui,item:CategoryTreeRow<'_>,product:bool,enabled:bool,draggable:bool)->Response{
    let (rect,response)=ui.shared_allocate_exact_size(
        Vec2::new(ui.available_width().max(1.0),theme::CATALOG_TREE_ROW_HEIGHT),
        crate::drag_drop::sense(enabled,draggable)
    );
    #[cfg(test)]
    if !product{crate::ui_test_support::record_highlight_target("category row",rect,ui.clip_rect());}
    let geo=CategoryTreeGeometry::new(rect,item.depth);
    let painter=ui.painter().with_shared_clip(rect);
    if item.selected{
        crate::highlight::paint(ui,rect,crate::highlight::Geometry::Control,crate::highlight::Role::Selected);
    }else if enabled{
        crate::button::paint_button_hover(ui,&response,crate::highlight::Geometry::Control);
    }
    let response=if product&&enabled{response.on_hover_cursor(egui::CursorIcon::PointingHand)}else{response};
    let guide=Stroke::new(CATALOG_TREE_GUIDE_WIDTH,CATALOG_TREE_GUIDE_COLOR);
    geo.guides(item.ancestor_guides,item.is_last,item.has_children,item.open,
        |segment|{painter.line_segment(segment,guide);});
    if item.has_children{
        paint_management_chevron(&painter,geo.chevron.center(),
            if item.open{ManagementChevronDirection::Down}else{ManagementChevronDirection::Right});
    }
    let ink=if item.selected{theme::category_ink(theme::CategoryInkRole::Current)}else{CATALOG_TREE_FOLDER_COLOR};
    if product{
        // A small parcel glyph occupies the exact shared folder slot.
        let p=|x:f32,y:f32|egui::pos2(geo.folder.left()+geo.folder.width()*x/20.0,
            geo.folder.top()+geo.folder.height()*y/20.0);
        let stroke=Stroke::new(CATALOG_TREE_FOLDER_STROKE,
            theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
        painter.add(egui::Shape::closed_line(vec![p(3.0,5.0),p(10.0,2.0),p(17.0,5.0),
            p(17.0,14.0),p(10.0,18.0),p(3.0,14.0)],stroke));
        painter.add(egui::Shape::line(vec![p(3.0,5.0),p(10.0,9.0),p(17.0,5.0)],stroke));
        painter.line_segment([p(10.0,9.0),p(10.0,18.0)],stroke);
    }else{
        paint_category_tree_folder(&painter,geo.folder,item.has_children&&item.open,ink);
    }
    // Reserve metadata first and clip both label regions at the shared inset.
    // No full-width row dividers: hierarchy rails own the grouping.
    let right=(rect.right()-CATALOG_TREE_INSET).max(rect.left());
    let name_x=geo.name_x.min(right);
    let summary_galley=typography::client_table_metadata_galley(ui,item.summary,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
    let summary_reserve=if item.summary.is_empty(){0.0}else{CATALOG_TREE_GAP+summary_galley.size().x};
    let name_width=(right-name_x-summary_reserve).max(0.0);
    let font=if item.root{typography::CategoryPathFont::Subsection}else{typography::CategoryPathFont::Body};
    let visible=typography::fit_category_path(ui,item.name,name_width,font);
    let name_galley=typography::category_tree_name_galley(ui,&visible,item.root);
    let name_end=name_x+name_galley.size().x.min(name_width);
    if name_end>name_x{
        painter.with_shared_clip(Rect::from_min_max(egui::pos2(name_x,rect.top()),
            egui::pos2(name_end,rect.bottom()))).galley(
            egui::pos2(name_x,rect.center().y-name_galley.size().y/2.0),
            name_galley,theme::text_ink(theme::TextRole::Value,theme::TextState::Normal));
    }
    let summary_x=(name_end+CATALOG_TREE_GAP).min(right);
    if !item.summary.is_empty()&&right>summary_x{
        painter.with_shared_clip(Rect::from_min_max(egui::pos2(summary_x,rect.top()),
            egui::pos2(right,rect.bottom()))).galley(
            egui::pos2(summary_x,rect.center().y-summary_galley.size().y/2.0),
            summary_galley,theme::text_ink(theme::TextRole::Label,theme::TextState::Normal));
    }
    overflow_tooltip(response,visible!=item.name,item.name)
}

pub(crate) fn empty_search_row(ui:&mut Ui,message:&str){
    ui.label(typography::caption(message));
}
#[cfg(test)]
mod category_tree_geometry_tests{
    use super::*;
    fn row(depth:usize)->CategoryTreeGeometry{
        CategoryTreeGeometry::new(Rect::from_min_size(egui::pos2(0.0,0.0),
            Vec2::new(500.0,theme::CATALOG_TREE_ROW_HEIGHT)),depth)
    }
    #[test]
    fn product_selection_has_unique_ids_and_unselected_drag_is_single(){
        let mut selection=ProductSelection::default();
        selection.toggle("p1","a");selection.toggle("p2","b");
        assert_eq!(selection.members("p1","a").len(),2);
        assert_eq!(selection.members("p3","c"),vec![("p3".into(),"c".into())]);
        selection.toggle("p1","a");
        assert!(!selection.contains("p1"));assert!(selection.contains("p2"));
        selection.clear();assert!(!selection.contains("p2"));
    }
    #[test]
    fn product_drag_hover_requires_continuous_target_and_resets_after_leave(){
        let mut hover=CategoryDragHover::default();
        hover.begin_frame();
        assert!(!hover.advance(DragKind::Product,"p","source","a",0.0));
        hover.end_frame();
        hover.begin_frame();
        assert!(!hover.advance(DragKind::Product,"p","source","a",0.64));
        hover.end_frame();
        hover.begin_frame();
        assert!(!hover.advance(DragKind::Product,"p","source","b",0.65));
        hover.end_frame();
        hover.begin_frame();
        hover.end_frame();
        hover.begin_frame();
        assert!(!hover.advance(DragKind::Product,"p","source","b",2.0));
        hover.end_frame();
        hover.begin_frame();
        assert!(hover.advance(DragKind::Product,"p","source","b",2.66));
    }
    #[test]
    fn folder_hover_uses_same_delay_but_does_not_inherit_product_timer(){
        let mut hover=CategoryDragHover::default();
        hover.begin_frame();
        assert!(!hover.advance(DragKind::Product,"a","parent","target",0.0));
        hover.end_frame();
        hover.begin_frame();
        assert!(!hover.advance(DragKind::Folder,"a","parent","target",0.64));
        hover.end_frame();
        hover.begin_frame();
        assert!(!hover.advance(DragKind::Folder,"a","parent","target",1.28));
        hover.end_frame();
        hover.begin_frame();
        assert!(hover.advance(DragKind::Folder,"a","parent","target",1.30));
    }
    #[test]
    fn folder_hover_and_drop_share_cycle_and_parent_guards(){
        let drag=FolderDrag{id:"a".into(),parent:"parent".into(),name:"폴더".into()};
        assert!(!valid_folder_target(&drag,"a",|_|false));
        assert!(!valid_folder_target(&drag,"parent",|_|false));
        assert!(!valid_folder_target(&drag,"child",|id|id=="a"));
        assert!(valid_folder_target(&drag,"other",|_|false));
        assert!(!valid_folder_target(&drag,"",|_|false));
    }
    #[test]
    fn category_tree_slots_share_depth_and_center(){
        let root=row(0);
        let child=row(1);
        assert_eq!(root.row.height(),42.0);
        assert_eq!(root.folder.size(),Vec2::splat(20.0));
        assert_eq!(root.chevron.size(),Vec2::splat(16.0));
        assert_eq!(child.folder.left()-root.folder.left(),24.0);
        assert_eq!(child.folder.left()-child.chevron.right(),8.0);
        assert_eq!(child.name_x-child.folder.right(),8.0);
        assert_eq!(child.folder.center().y,child.chevron.center().y);
    }
    #[test]
    fn category_tree_leaf_elbow_reaches_folder_and_last_rail_stops(){
        let g=row(2);
        let mut lines=Vec::new();
        g.guides(&[true],true,false,false,|s|lines.push(s));
        assert_eq!(lines.len(),3);
        assert_eq!(lines[0][0].x,g.axis(0));
        assert_eq!(lines[0][1].y,g.row.bottom());
        assert_eq!(lines[1][1].y,g.row.center().y);
        assert_eq!(lines[2][1].x,g.folder.left()-4.0);
        lines.clear();
        g.guides(&[false],false,true,true,|s|lines.push(s));
        assert_eq!(lines.len(),3);
        assert_eq!(lines[0][1].y,g.row.bottom());
        assert_eq!(lines[1][1].x,g.chevron.left()-4.0);
        assert_eq!(lines[2][0].x,g.axis(2));
        assert_eq!(lines[2][1].y,g.row.bottom());
    }
}

/// Geometry of the shared category explorer; motion remains screen-owned.
pub(crate) fn configure_explorer_popup(ui:&mut Ui,width:f32){
    ui.apply_shared_width(crate::ui_layout::geometry::CardWidthPolicy::Bounded,width);
    ui.spacing_mut().item_spacing.y=theme::SPACE_8;
}
pub(crate) fn configure_tree_rows(ui:&mut Ui){
    ui.spacing_mut().item_spacing.y=0.0;
}
pub(crate) fn configure_breadcrumb_row(ui:&mut Ui){
    ui.spacing_mut().item_spacing.x=0.0;
}
