//! Shared category tree geometry, rendering and interaction surface.
use crate::ui_layout::geometry::UiWidthPolicyExt as _;
use crate::ui_layout::geometry::SharedPainterClipExt as _;
use crate::ui_layout::geometry::SharedAllocationExt as _;
use eframe::egui::{self,Color32,Rect,Response,Sense,Stroke,Ui,Vec2};
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
const CATALOG_TREE_FOLDER_COLOR:Color32 = theme::SECONDARY;

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

/// Product leaf opens the existing product detail; it never changes
/// category selection or expansion.
pub(crate) fn product_tree_row(ui:&mut Ui,name:&str,depth:usize,ancestor_guides:&[bool],is_last:bool,enabled:bool,draggable:bool)->Response{
    tree_row(ui,CategoryTreeRow{
        name,summary:"",depth,ancestor_guides,is_last,
        root:false,has_children:false,open:false,selected:false,
    },true,enabled,draggable)
}

#[derive(Clone)]
struct ProductDrag { id:String, source:String }

pub(crate) struct ProductMove {
    pub(crate) id:String,
    pub(crate) source:String,
    pub(crate) target:String,
}

pub(crate) fn start_product_drag(ui:&Ui,response:&Response,id:&str,source:&str,enabled:bool){
    if enabled&&ui.is_enabled()&&!id.is_empty()&&!source.is_empty(){
        response.dnd_set_drag_payload(ProductDrag{id:id.to_owned(),source:source.to_owned()});
        if response.dragged(){ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);}
    }
}

/// Native typed drag/drop; only real folders are accepted. The source
/// remains untouched until ProductManager commits a Rust mutation.
pub(crate) fn product_drop_target(ui:&Ui,response:&Response,target:&str,enabled:bool)->Option<ProductMove>{
    if !enabled||!ui.is_enabled()||target.is_empty(){return None;}
    let valid=response.dnd_hover_payload::<ProductDrag>()
        .is_some_and(|drag|drag.source!=target);
    if valid{
        crate::highlight::paint(ui,response.rect.intersect(ui.clip_rect()),
            crate::highlight::Geometry::Control,crate::highlight::Role::DropHovered);
    }
    response.dnd_release_payload::<ProductDrag>().filter(|drag|drag.source!=target)
        .map(|drag|ProductMove{id:drag.id.clone(),source:drag.source.clone(),target:target.to_owned()})
}

fn tree_row(ui:&mut Ui,item:CategoryTreeRow<'_>,product:bool,enabled:bool,draggable:bool)->Response{
    let (rect,response)=ui.shared_allocate_exact_size(
        Vec2::new(ui.available_width().max(1.0),theme::CATALOG_TREE_ROW_HEIGHT),
        if enabled&&draggable{Sense::click_and_drag()}else if enabled{Sense::click()}else{Sense::hover()}
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
    let ink=if item.selected{theme::CATALOG_TREE_SELECTED_INK}else{CATALOG_TREE_FOLDER_COLOR};
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
