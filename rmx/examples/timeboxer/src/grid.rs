//! `schedule-grid.tsx`: the weekly grid. Click a cell to add a block, type to name it,
//! drag blocks (shift-click selects a run on one day and drags it as a group), drag the top or
//! bottom edge to resize, drag a side edge to copy the block across days, Backspace to delete.
//! Every committed change saves the whole schedule document with `PUT /schedules/:id`.

use rmx::browser::{self, FetchInit};
use rmx::{css, on, rsx, Component, Handle, Mixin, Node, NodeHandle};
use rmx::ui::animation::{animate_layout, Layout, SpringPreset};
use serde::{Deserialize, Serialize};

use crate::dom_util::{self, Pointer, WindowListeners};
use crate::layout::{
    js_round, preview_copy_block_across_days, preview_delete_block, preview_move_block, preview_move_block_group, preview_resize_block_time, Block,
    LayoutResult, Placement, Policy, ResizeEdge, ScheduleDocument,
};
use crate::theme::{self, border, font_size, space, surface, text, Tone, FOCUS_RING, RADIUS};

pub const WEEK_DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const FIRST_SLOT_MINUTE: i64 = 0;
const INITIAL_SCROLL_MINUTE: i64 = 6 * 60 + 30;
const SLOT_MINUTES: i64 = 15;
const LAST_SLOT_MINUTE: i64 = 24 * 60;
const SLOT_COUNT: i64 = (LAST_SLOT_MINUTE - FIRST_SLOT_MINUTE) / SLOT_MINUTES;
const GRID_LABEL_WIDTH: f64 = 72.0;
const GRID_ROW_HEIGHT: f64 = 36.0;
const DRAG_THRESHOLD: f64 = 4.0;
const HORIZONTAL_RESIZE_THRESHOLD: f64 = 0.15;

fn policy() -> Policy {
    Policy { day_minutes: LAST_SLOT_MINUTE, minimum_minute: FIRST_SLOT_MINUTE, slot_minutes: SLOT_MINUTES, ..Policy::default() }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleGridProps {
    pub csrf_token: String,
    pub download_ics_href: String,
    pub schedule: ScheduleDocument,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GridMeasurement {
    day_width: f64,
    label_width: f64,
    left: f64,
    row_height: f64,
    top: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayEdge {
    DayStart,
    DayEnd,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Gesture {
    Drag,
    Resize,
    HorizontalResize,
}

struct DragState {
    active: bool,
    block_id: String,
    block_ids: Vec<String>,
    duration: i64,
    grid: GridMeasurement,
    moved: bool,
    offset_x: f64,
    offset_y: f64,
    original_blocks: Vec<Block>,
    placement: Placement,
    pointer_id: i64,
    start_x: f64,
    start_y: f64,
}

struct ResizeState {
    block_id: String,
    edge: ResizeEdge,
    grid: GridMeasurement,
    moved: bool,
    offset_y: f64,
    original_block: Block,
    original_blocks: Vec<Block>,
    pointer_id: i64,
    start_y: f64,
}

struct HorizontalResizeState {
    active_block_id: String,
    block_id: String,
    edge: DayEdge,
    grid: GridMeasurement,
    id_prefix: String,
    moved: bool,
    offset_x: f64,
    original_block: Block,
    original_blocks: Vec<Block>,
    pointer_id: i64,
    start_x: f64,
}

struct DragPreview {
    placement: Placement,
    visual_offset: (f64, f64),
}

pub struct ScheduleGrid {
    schedule: ScheduleDocument,
    draft_id: Option<String>,
    drag: Option<DragState>,
    resize: Option<ResizeState>,
    horizontal_resize: Option<HorizontalResizeState>,
    preview: Option<LayoutResult>,
    grid_el: Option<NodeHandle>,
    scroller_el: Option<NodeHandle>,
    gesture: Option<(Gesture, WindowListeners)>,
    horizontal_resize_sequence: u32,
    save_sequence: u64,
    latest_applied_save_sequence: u64,
    /// In selection order (`Set` iteration order in Remix).
    selected: Vec<String>,
    selection_anchor: Option<String>,
    suppress_draft_click_until: f64,
    suppress_selection_click_until: f64,
}

impl Component for ScheduleGrid {
    type Props = ScheduleGridProps;
    rmx::client_entry!("ScheduleGrid");

    fn setup(props: &ScheduleGridProps, h: &Handle<Self>) -> Self {
        h.queue_task(|this, _| this.scroll_to_initial_time());
        ScheduleGrid {
            schedule: props.schedule.clone(),
            draft_id: None,
            drag: None,
            resize: None,
            horizontal_resize: None,
            preview: None,
            grid_el: None,
            scroller_el: None,
            gesture: None,
            horizontal_resize_sequence: 0,
            save_sequence: 0,
            latest_applied_save_sequence: 0,
            selected: Vec::new(),
            selection_anchor: None,
            suppress_draft_click_until: 0.0,
            suppress_selection_click_until: 0.0,
        }
    }

    fn render(&self, props: &ScheduleGridProps, h: &Handle<Self>) -> Node {
        let visible = self.preview.as_ref().map_or(&self.schedule.blocks, |p| &p.blocks);
        let dragging = self.drag.as_ref().filter(|d| d.active);
        let ghosts: Vec<Node> = dragging
            .map(|d| visible.iter().filter(|b| d.block_ids.contains(&b.id)).map(ghost).collect())
            .unwrap_or_default();
        let blocks: Vec<Node> = visible
            .iter()
            .map(|block| {
                rsx! {
                    <ScheduleBlock key={block.id.clone()}
                        block={block.clone()}
                        active_day_edge={self.horizontal_resize.as_ref().map(|s| s.edge)}
                        active_resize_edge={self.resize.as_ref().map(|s| s.edge)}
                        is_dragging={dragging.is_some_and(|d| d.block_ids.contains(&block.id))}
                        is_draft={self.draft_id.as_deref() == Some(block.id.as_str())}
                        is_horizontal_resizing={self.horizontal_resize.as_ref().is_some_and(|s| s.active_block_id == block.id)}
                        is_resizing={self.resize.as_ref().is_some_and(|s| s.block_id == block.id)}
                        is_selected={self.selected.contains(&block.id)}
                        should_animate_layout={dragging.is_some_and(|d| !d.block_ids.contains(&block.id))}
                        grid={h.clone()}/>
                }
            })
            .collect();
        let resizing = if self.resize.is_some() {
            Some("vertical")
        } else if self.horizontal_resize.is_some() {
            Some("horizontal")
        } else {
            None
        };
        let columns = format!("grid-template-columns: {GRID_LABEL_WIDTH}px repeat(7, minmax(88px, 1fr));");
        let day_headers: Vec<Node> = WEEK_DAYS
            .iter()
            .map(|day| {
                rsx! { <div key={*day} mix={css(&format!("color: {}; font-size: {}; font-weight: 500; padding: 2px {}; text-align: center;", text::PRIMARY, font_size::XS, space::XS))}>{*day}</div> }
            })
            .collect();
        rsx! {
            <section aria-label="Weekly schedule" data-schedule-dragging={dragging.map(|_| "true")} data-schedule-resizing={resizing} mix={week_schedule_style()}>
                <div mix={css(&format!("align-items: center; display: grid; grid-template-columns: 1fr auto 1fr; gap: {}; padding: 0 {};", space::SM, space::MD))}>
                    <div aria-hidden="true"></div>
                    <div mix={css(&format!("color: {}; font-size: {}; font-weight: 700; justify-content: center; letter-spacing: {};", text::PRIMARY, font_size::XL, theme::LETTER_SPACING_TIGHT))}>{self.schedule.name.clone()}</div>
                    <div mix={css(&format!("align-items: center; display: flex; flex-wrap: wrap; gap: {}; justify-self: end;", space::SM))}>
                        <a download href={props.download_ics_href.clone()} mix={theme::button(Tone::Neutral)}>"Download ICS"</a>
                    </div>
                </div>
                <div mix={css(&format!("align-items: end; border-bottom: 1px solid {}; display: grid; {columns} padding-right: 10px;", border::STRONG))}>
                    <div aria-hidden="true"></div>
                    {day_headers}
                </div>
                <div mix={[css("min-height: 0; overflow-y: auto;"), h.node_ref(|this, node| this.scroller_el = Some(node.clone()))]}>
                    <div mix={[css("min-width: 760px; position: relative;"), h.node_ref(|this, node| this.grid_el = Some(node.clone()))]}>
                        {time_rows(h)}
                        <div aria-label="Scheduled blocks" mix={css(&format!("display: grid; {columns} grid-auto-rows: {GRID_ROW_HEIGHT}px; inset: 0; pointer-events: none; position: absolute; z-index: 1;"))}>
                            {ghosts}
                            {blocks}
                        </div>
                    </div>
                </div>
            </section>
        }
    }
}

/// The time labels and the clickable cells behind the blocks.
fn time_rows(h: &Handle<ScheduleGrid>) -> Node {
    let row = css(&format!("display: grid; grid-template-columns: {GRID_LABEL_WIDTH}px repeat(7, minmax(88px, 1fr)); min-height: {GRID_ROW_HEIGHT}px;"));
    let spacers: Vec<Node> = WEEK_DAYS
        .iter()
        .map(|day| rsx! { <div key={format!("spacer-{day}")} mix={css(&format!("border-left: 1px dashed {}; min-height: {GRID_ROW_HEIGHT}px;", border::DEFAULT))}></div> })
        .collect();
    let cell = css(&format!("border-left: 1px dashed {d}; border-top: 1px dashed {d}; min-height: {GRID_ROW_HEIGHT}px;", d = border::DEFAULT));
    let hour_cell = css(&format!("border-top: 1px solid {};", border::STRONG));
    let half_hour_cell = css(&format!("border-top: 1px dotted {};", border::STRONG));
    let label = css(&format!("color: {}; font-size: 0.6875rem; padding: 0 {} 0 0; text-align: right; transform: translateY(-0.7em);", text::SECONDARY, space::XS));
    let hour_label = css(&format!("color: {}; font-weight: 600;", text::PRIMARY));
    let rows: Vec<Node> = (0..SLOT_COUNT)
        .map(|slot| {
            let minute = FIRST_SLOT_MINUTE + slot * SLOT_MINUTES;
            let time = format_time_slot(minute);
            let is_hour = minute % 60 == 0;
            let is_half = minute % 60 == 30;
            let cells: Vec<Node> = (0..7i64)
                .map(|day| {
                    let pointer_down = h.on_event(|this, h, event| {
                        if Pointer::from_event(event).button == 0 && this.clear_block_focus_or_selection(h) {
                            this.suppress_draft_click_until = browser::now() + 250.0;
                        }
                    });
                    let click = h.on(move |this, h| {
                        if browser::now() < this.suppress_draft_click_until || this.clear_block_focus_or_selection(h) {
                            return;
                        }
                        this.start_draft(h, day, minute);
                    });
                    rsx! {
                        <div aria-label={format!("{} {time}", WEEK_DAYS[day as usize])} key={format!("{day}-{minute}")}
                            mix={[cell.clone(), on("pointerdown", pointer_down), on("click", click), if is_hour { hour_cell.clone() } else if is_half { half_hour_cell.clone() } else { Mixin::from(Vec::new()) }]}></div>
                    }
                })
                .collect();
            rsx! {
                <div key={minute} mix={row.clone()}>
                    <div mix={[label.clone(), if is_hour { hour_label.clone() } else { Mixin::from(Vec::new()) }]}>{time}</div>
                    {cells}
                </div>
            }
        })
        .collect();
    rsx! {
        <div mix={css("position: relative;")}>
            <div aria-hidden="true" mix={row.clone()}>
                <div></div>
                {spacers}
            </div>
            {rows}
        </div>
    }
}

fn ghost(block: &Block) -> Node {
    rsx! {
        <div aria-hidden="true" key={format!("drop-ghost-{}", block.id)} style={grid_area(block)}
            mix={css("display: grid; min-height: 0; min-width: 0; pointer-events: none; position: relative; z-index: 1;")}>
            <div mix={css(&format!("background-color: rgb(209 213 219 / 0.72); border: 2px dashed {}; border-radius: {RADIUS}; margin: 2px 4px; opacity: 0.5;", text::SECONDARY))}></div>
        </div>
    }
}

fn grid_area(block: &Block) -> String {
    format!(
        "grid-column: {}; grid-row: {} / span {}",
        block.day_of_week + 2,
        start_minute_to_slot_index(block.start_minute) + 2,
        duration_to_slot_span(block.start_minute, block.end_minute)
    )
}

impl ScheduleGrid {
    fn visible_blocks(&self) -> &[Block] {
        self.preview.as_ref().map_or(&self.schedule.blocks, |p| &p.blocks)
    }

    fn select_only(&mut self, id: &str) {
        self.selected = vec![id.to_string()];
        self.selection_anchor = Some(id.to_string());
    }

    fn start_draft(&mut self, h: &Handle<Self>, day: i64, start_minute: i64) {
        if self.draft_id.is_some() {
            return;
        }
        let id = format!("block-{}", dom_util::date_now() as i64);
        self.schedule.blocks.push(Block {
            color: None,
            day_of_week: day,
            end_minute: start_minute + SLOT_MINUTES,
            id: id.clone(),
            name: String::new(),
            start_minute,
        });
        self.draft_id = Some(id);
        h.update();
    }

    fn cancel_draft(&mut self, h: &Handle<Self>) {
        if let Some(id) = self.draft_id.take() {
            self.schedule.blocks.retain(|b| b.id != id);
        }
        h.update();
    }

    fn commit_block(&mut self, h: &Handle<Self>, id: &str, name: &str) {
        let name = name.trim();
        let is_draft = self.draft_id.as_deref() == Some(id);
        if name.is_empty() {
            if is_draft {
                self.cancel_draft(h);
            }
            return;
        }
        if let Some(block) = self.schedule.blocks.iter_mut().find(|b| b.id == id) {
            block.name = name.to_string();
        }
        if is_draft {
            self.draft_id = None;
        }
        h.update();
        self.save_schedule(h);
    }

    fn delete_block(&mut self, h: &Handle<Self>, id: &str) {
        if self.draft_id.as_deref() == Some(id) {
            self.cancel_draft(h);
            return;
        }
        let delete_ids = if self.selected.iter().any(|s| s == id) { self.selected.clone() } else { vec![id.to_string()] };
        let mut blocks = self.schedule.blocks.clone();
        for id in &delete_ids {
            blocks = preview_delete_block(&blocks, id).blocks;
        }
        self.schedule.blocks = blocks;
        self.selected.retain(|s| !delete_ids.contains(s));
        if self.selection_anchor.as_ref().is_some_and(|a| delete_ids.contains(a)) {
            self.selection_anchor = None;
        }
        h.update();
        self.save_schedule(h);
    }

    fn select_block(&mut self, h: &Handle<Self>, id: &str, shift: bool) {
        if self.draft_id.is_some() || self.gesture.is_some() {
            return;
        }
        if !shift && browser::now() < self.suppress_selection_click_until {
            return;
        }
        match (&self.selection_anchor, shift) {
            (Some(anchor), true) => self.selected = adjacent_selection(&self.schedule.blocks, anchor, id),
            _ => self.select_only(id),
        }
        h.update();
    }

    fn focus_block(&mut self, h: &Handle<Self>, id: &str) {
        if self.draft_id.is_some() || self.gesture.is_some() || self.selected.iter().any(|s| s == id) {
            return;
        }
        self.select_only(id);
        h.update();
    }

    fn clear_block_focus_or_selection(&mut self, h: &Handle<Self>) -> bool {
        if dom_util::focused_block_id().is_none() && self.selected.is_empty() {
            return false;
        }
        self.selected.clear();
        self.selection_anchor = None;
        dom_util::blur_active();
        h.update();
        true
    }

    fn clear_other_block_focus_or_selection(&mut self, h: &Handle<Self>, id: &str) -> bool {
        let focused = dom_util::focused_block_id();
        let had_other = !self.selected.is_empty() && !(self.selected.len() == 1 && self.selected[0] == id);
        if !had_other && (focused.as_deref() == Some(id) || focused.is_none()) {
            return false;
        }
        self.selected.clear();
        self.selection_anchor = None;
        if focused.as_deref() != Some(id) {
            dom_util::blur_active();
        }
        h.update();
        true
    }

    /// `prepareBlockPointerDown` then `startDrag`.
    fn block_pointer_down(&mut self, h: &Handle<Self>, block: &Block, pointer: Pointer) {
        if self.draft_id.is_none() && self.gesture.is_none() && !self.selected.contains(&block.id) {
            self.select_only(&block.id);
            h.update();
        }
        self.start_drag(h, block, pointer);
    }

    fn start_drag(&mut self, h: &Handle<Self>, block: &Block, pointer: Pointer) {
        if self.draft_id.is_some() || self.gesture.is_some() || pointer.button != 0 {
            return;
        }
        let Some(grid) = self.measure() else { return };
        let block_left = grid.left + grid.label_width + block.day_of_week as f64 * grid.day_width;
        let block_top = grid.top + (start_minute_to_slot_index(block.start_minute) + 1) as f64 * grid.row_height;
        let block_ids = if self.selected.contains(&block.id) { self.selected.clone() } else { vec![block.id.clone()] };
        if !self.selected.contains(&block.id) {
            self.selected = block_ids.clone();
            self.selection_anchor = Some(block.id.clone());
        }
        self.drag = Some(DragState {
            active: true,
            block_id: block.id.clone(),
            block_ids,
            duration: block.end_minute - block.start_minute,
            grid,
            moved: false,
            offset_x: pointer.x - block_left,
            offset_y: pointer.y - block_top,
            original_blocks: self.schedule.blocks.clone(),
            placement: Placement { day_of_week: block.day_of_week, start_minute: block.start_minute },
            pointer_id: pointer.id,
            start_x: pointer.x,
            start_y: pointer.y,
        });
        self.bind_gesture(h, Gesture::Drag);
        h.update();
    }

    fn move_drag(&mut self, h: &Handle<Self>, pointer: &Pointer) -> bool {
        let Some(drag) = self.drag.as_mut().filter(|d| d.pointer_id == pointer.id) else { return false };
        let distance = (pointer.x - drag.start_x).hypot(pointer.y - drag.start_y);
        if !drag.moved && distance < DRAG_THRESHOLD {
            return false;
        }
        drag.moved = true;
        let next = pointer_to_drag_preview(pointer, drag);
        if next.placement == drag.placement {
            let (ids, offset) = (drag.block_ids.clone(), next.visual_offset);
            self.set_drag_offset(&ids, Some(offset));
            return true;
        }
        let preview = if drag.block_ids.len() > 1 {
            preview_move_block_group(&drag.original_blocks, &drag.block_ids, &drag.block_id, next.placement, &policy())
        } else {
            preview_move_block(&drag.original_blocks, &drag.block_id, next.placement, &policy())
        };
        let ids = drag.block_ids.clone();
        if preview.unresolved {
            let offset = visual_offset_from_placement(&next, drag);
            self.set_drag_offset(&ids, Some(offset));
            return true;
        }
        drag.placement = next.placement;
        self.set_drag_offset(&ids, Some(next.visual_offset));
        self.update_preview(h, preview);
        true
    }

    fn end_drag(&mut self, h: &Handle<Self>, pointer: &Pointer) -> bool {
        if !self.drag.as_ref().is_some_and(|d| d.pointer_id == pointer.id) {
            return false;
        }
        self.gesture = None;
        let drag = self.drag.take().expect("checked above");
        if drag.moved {
            self.suppress_selection_click_until = browser::now() + 250.0;
        }
        self.set_drag_offset(&drag.block_ids, None);
        self.finish_gesture(h, drag.moved, Some(drag.block_ids))
    }

    /// Apply a resolved preview and save, or drop it. Whether to `preventDefault()`.
    fn finish_gesture(&mut self, h: &Handle<Self>, moved: bool, select: Option<Vec<String>>) -> bool {
        let preview = self.preview.take();
        h.update();
        match preview {
            Some(preview) if moved && !preview.unresolved => {
                self.schedule.blocks = preview.blocks;
                if let Some(ids) = select {
                    self.selected = ids;
                }
                self.save_schedule(h);
                true
            }
            _ => false,
        }
    }

    fn start_resize(&mut self, h: &Handle<Self>, block: &Block, edge: ResizeEdge, pointer: Pointer) {
        if self.draft_id.as_ref().is_some_and(|d| *d != block.id) || self.drag.is_some() || self.horizontal_resize.is_some() || pointer.button != 0 {
            return;
        }
        let Some(grid) = self.measure() else { return };
        self.clear_other_block_focus_or_selection(h, &block.id);
        let edge_minute = if edge == ResizeEdge::Start { block.start_minute } else { block.end_minute };
        let edge_top = grid.top + (start_minute_to_slot_index(edge_minute) + 1) as f64 * grid.row_height;
        self.resize = Some(ResizeState {
            block_id: block.id.clone(),
            edge,
            grid,
            moved: false,
            offset_y: pointer.y - edge_top,
            original_block: block.clone(),
            original_blocks: self.schedule.blocks.clone(),
            pointer_id: pointer.id,
            start_y: pointer.y,
        });
        self.bind_gesture(h, Gesture::Resize);
        h.update();
    }

    fn move_resize(&mut self, h: &Handle<Self>, pointer: &Pointer) -> bool {
        let Some(state) = self.resize.as_mut().filter(|s| s.pointer_id == pointer.id) else { return false };
        if !state.moved && (pointer.y - state.start_y).abs() < DRAG_THRESHOLD {
            return false;
        }
        state.moved = true;
        let minute = pointer_to_resize_minute(pointer, state);
        let preview = preview_resize_block_time(&state.original_blocks, &state.block_id, state.edge, minute as f64, &policy());
        self.update_preview(h, preview);
        true
    }

    fn end_resize(&mut self, h: &Handle<Self>, pointer: &Pointer) -> bool {
        if !self.resize.as_ref().is_some_and(|s| s.pointer_id == pointer.id) {
            return false;
        }
        self.gesture = None;
        let state = self.resize.take().expect("checked above");
        self.finish_gesture(h, state.moved, None)
    }

    fn start_horizontal_resize(&mut self, h: &Handle<Self>, block: &Block, edge: DayEdge, pointer: Pointer) {
        if self.draft_id.as_ref().is_some_and(|d| *d != block.id)
            || self.drag.is_some()
            || self.horizontal_resize.is_some()
            || self.resize.is_some()
            || pointer.button != 0
        {
            return;
        }
        let Some(grid) = self.measure() else { return };
        self.clear_other_block_focus_or_selection(h, &block.id);
        let edge_column = if edge == DayEdge::DayStart { block.day_of_week } else { block.day_of_week + 1 };
        let edge_left = grid.left + grid.label_width + edge_column as f64 * grid.day_width;
        self.horizontal_resize_sequence += 1;
        self.horizontal_resize = Some(HorizontalResizeState {
            active_block_id: block.id.clone(),
            block_id: block.id.clone(),
            edge,
            grid,
            id_prefix: format!("repeat-{}-{}", self.horizontal_resize_sequence, to_base36(dom_util::date_now() as u64)),
            moved: false,
            offset_x: pointer.x - edge_left,
            original_block: block.clone(),
            original_blocks: self.schedule.blocks.clone(),
            pointer_id: pointer.id,
            start_x: pointer.x,
        });
        self.bind_gesture(h, Gesture::HorizontalResize);
        h.update();
    }

    fn move_horizontal_resize(&mut self, h: &Handle<Self>, pointer: &Pointer) -> bool {
        let Some(state) = self.horizontal_resize.as_mut().filter(|s| s.pointer_id == pointer.id) else { return false };
        if !state.moved && (pointer.x - state.start_x).abs() < DRAG_THRESHOLD {
            return false;
        }
        state.moved = true;
        let day = pointer_to_resize_day(pointer, state);
        let original_day = state.original_block.day_of_week;
        let first = if state.edge == DayEdge::DayStart { day } else { original_day };
        let last = if state.edge == DayEdge::DayEnd { day } else { original_day };
        let active_day = if state.edge == DayEdge::DayStart { first } else { last };
        state.active_block_id = if active_day == original_day { state.block_id.clone() } else { format!("{}-{active_day}", state.id_prefix) };
        let prefix = state.id_prefix.clone();
        let preview = preview_copy_block_across_days(&state.original_blocks, &state.block_id, first, last, |_, day| format!("{prefix}-{day}"), &policy());
        if !self.update_preview(h, preview) {
            // The active block id may still have changed.
            h.update();
        }
        true
    }

    fn end_horizontal_resize(&mut self, h: &Handle<Self>, pointer: &Pointer) -> bool {
        if !self.horizontal_resize.as_ref().is_some_and(|s| s.pointer_id == pointer.id) {
            return false;
        }
        self.gesture = None;
        let state = self.horizontal_resize.take().expect("checked above");
        self.finish_gesture(h, state.moved, None)
    }

    fn bind_gesture(&mut self, h: &Handle<Self>, kind: Gesture) {
        let (on_move, on_end) = (h.clone(), h.clone());
        let listeners = WindowListeners::bind(
            move |p| {
                on_move
                    .with(|this, h| match this.gesture.as_ref().map(|g| g.0) {
                        Some(Gesture::Drag) => this.move_drag(h, p),
                        Some(Gesture::Resize) => this.move_resize(h, p),
                        Some(Gesture::HorizontalResize) => this.move_horizontal_resize(h, p),
                        None => false,
                    })
                    .unwrap_or(false)
            },
            move |p| {
                on_end
                    .with(|this, h| match this.gesture.as_ref().map(|g| g.0) {
                        Some(Gesture::Drag) => this.end_drag(h, p),
                        Some(Gesture::Resize) => this.end_resize(h, p),
                        Some(Gesture::HorizontalResize) => this.end_horizontal_resize(h, p),
                        None => false,
                    })
                    .unwrap_or(false)
            },
        );
        self.gesture = Some((kind, listeners));
    }

    /// Show a new preview; false when it matches what is already shown.
    fn update_preview(&mut self, h: &Handle<Self>, preview: LayoutResult) -> bool {
        if self.visible_blocks() == preview.blocks.as_slice() {
            return false;
        }
        self.preview = Some(preview);
        h.update();
        true
    }

    fn set_drag_offset(&self, ids: &[String], offset: Option<(f64, f64)>) {
        if let Some(grid) = &self.grid_el {
            let translate = offset.map_or_else(|| "none".to_string(), |(x, y)| format!("{x:.2}px {y:.2}px"));
            dom_util::set_block_translate(grid, ids, &translate);
        }
    }

    fn measure(&self) -> Option<GridMeasurement> {
        let rect = dom_util::rect(self.grid_el.as_ref()?);
        let row_height = if rect.height > 0.0 { rect.height / (SLOT_COUNT + 1) as f64 } else { GRID_ROW_HEIGHT }.max(1.0);
        Some(GridMeasurement {
            day_width: ((rect.width - GRID_LABEL_WIDTH) / 7.0).max(1.0),
            label_width: GRID_LABEL_WIDTH,
            left: rect.left,
            row_height,
            top: rect.top,
        })
    }

    fn scroll_to_initial_time(&self) {
        let (Some(grid), Some(scroller)) = (self.measure(), &self.scroller_el) else { return };
        dom_util::set_scroll_top(scroller, (start_minute_to_slot_index(INITIAL_SCROLL_MINUTE) + 1) as f64 * grid.row_height);
    }

    /// PUT the whole document; apply the response unless a newer save has started.
    fn save_schedule(&mut self, h: &Handle<Self>) {
        self.save_sequence += 1;
        let sequence = self.save_sequence;
        let schedule_id = self.schedule.id;
        let body = serde_json::json!({ "baseRevision": self.schedule.revision, "blocks": self.schedule.blocks, "name": self.schedule.name });
        let csrf_token = h.props().map(|p| p.csrf_token).unwrap_or_default();
        let h = h.clone();
        h.clone().spawn(async move {
            let init = FetchInit {
                method: "PUT".into(),
                headers: vec![("Content-Type".into(), "application/json".into()), ("X-Csrf-Token".into(), csrf_token)],
                body: Some(body.to_string()),
                ..Default::default()
            };
            let Ok(response) = browser::fetch(&crate::routes::schedule_href(schedule_id), init).await else { return };
            #[derive(Deserialize)]
            struct Saved {
                schedule: ScheduleDocument,
            }
            let Ok(saved) = response.json::<Saved>() else { return };
            h.with(|this, h| {
                if !response.ok() || sequence < this.latest_applied_save_sequence || this.schedule.id != schedule_id || sequence < this.save_sequence {
                    return;
                }
                this.latest_applied_save_sequence = sequence;
                this.schedule = saved.schedule;
                h.update();
            });
        });
    }
}

fn pointer_to_drag_preview(pointer: &Pointer, drag: &DragState) -> DragPreview {
    let g = &drag.grid;
    let raw_day = (pointer.x - drag.offset_x - g.left - g.label_width) / g.day_width;
    let raw_slot = (pointer.y - drag.offset_y - g.top - g.row_height) / g.row_height;
    let day = (js_round(raw_day) as i64).clamp(0, 6);
    let start = clamp_to_slot(FIRST_SLOT_MINUTE as f64 + js_round(raw_slot) * SLOT_MINUTES as f64, FIRST_SLOT_MINUTE, LAST_SLOT_MINUTE - drag.duration);
    let slot = (start - FIRST_SLOT_MINUTE) as f64 / SLOT_MINUTES as f64;
    DragPreview {
        placement: Placement { day_of_week: day, start_minute: start },
        visual_offset: ((raw_day - day as f64) * g.day_width, (raw_slot - slot) * g.row_height),
    }
}

fn visual_offset_from_placement(preview: &DragPreview, drag: &DragState) -> (f64, f64) {
    let placement_slot = (drag.placement.start_minute - FIRST_SLOT_MINUTE) as f64 / SLOT_MINUTES as f64;
    let preview_slot = (preview.placement.start_minute - FIRST_SLOT_MINUTE) as f64 / SLOT_MINUTES as f64;
    (
        preview.visual_offset.0 + (preview.placement.day_of_week - drag.placement.day_of_week) as f64 * drag.grid.day_width,
        preview.visual_offset.1 + (preview_slot - placement_slot) * drag.grid.row_height,
    )
}

fn pointer_to_resize_minute(pointer: &Pointer, state: &ResizeState) -> i64 {
    let g = &state.grid;
    let edge_top = pointer.y - state.offset_y;
    let raw = FIRST_SLOT_MINUTE as f64 + js_round((edge_top - g.top - g.row_height) / g.row_height) * SLOT_MINUTES as f64;
    match state.edge {
        ResizeEdge::Start => clamp_to_slot(raw, FIRST_SLOT_MINUTE, state.original_block.end_minute - SLOT_MINUTES),
        ResizeEdge::End => clamp_to_slot(raw, state.original_block.start_minute + SLOT_MINUTES, LAST_SLOT_MINUTE),
    }
}

fn pointer_to_resize_day(pointer: &Pointer, state: &HorizontalResizeState) -> i64 {
    let g = &state.grid;
    let raw_column = (pointer.x - state.offset_x - g.left - g.label_width) / g.day_width;
    let day = state.original_block.day_of_week;
    match state.edge {
        DayEdge::DayStart => ((raw_column + HORIZONTAL_RESIZE_THRESHOLD).floor() as i64).clamp(0, day),
        DayEdge::DayEnd => ((raw_column - HORIZONTAL_RESIZE_THRESHOLD).ceil() as i64 - 1).clamp(day, 6),
    }
}

/// Shift-click selection: every block between the anchor and the target on that day.
fn adjacent_selection(blocks: &[Block], anchor_id: &str, target_id: &str) -> Vec<String> {
    let only = || vec![target_id.to_string()];
    let (Some(anchor), Some(target)) = (blocks.iter().find(|b| b.id == anchor_id), blocks.iter().find(|b| b.id == target_id)) else { return only() };
    if anchor.day_of_week != target.day_of_week {
        return only();
    }
    let mut day: Vec<&Block> = blocks.iter().filter(|b| b.day_of_week == target.day_of_week).collect();
    day.sort_by(|l, r| l.start_minute.cmp(&r.start_minute).then_with(|| l.id.cmp(&r.id)));
    let (Some(a), Some(t)) = (day.iter().position(|b| b.id == anchor_id), day.iter().position(|b| b.id == target_id)) else { return only() };
    day[a.min(t)..=a.max(t)].iter().map(|b| b.id.clone()).collect()
}

fn clamp_to_slot(value: f64, min: i64, max: i64) -> i64 {
    let snapped = (js_round(value / SLOT_MINUTES as f64) * SLOT_MINUTES as f64) as i64;
    snapped.clamp(min, max.max(min))
}

fn start_minute_to_slot_index(minute: i64) -> i64 {
    (js_round((minute - FIRST_SLOT_MINUTE) as f64 / SLOT_MINUTES as f64) as i64).max(0)
}

fn duration_to_slot_span(start: i64, end: i64) -> i64 {
    (js_round((end - start) as f64 / SLOT_MINUTES as f64) as i64).max(1)
}

/// `9:00am`, `12:15pm`
pub fn format_time_slot(minute_of_day: i64) -> String {
    let hour24 = (minute_of_day / 60) % 24;
    let period = if hour24 < 12 { "am" } else { "pm" };
    let hour12 = if hour24 % 12 == 0 { 12 } else { hour24 % 12 };
    format!("{hour12}:{:02}{period}", minute_of_day % 60)
}

/// A pastel color from the block's name (or id), stable across renders.
fn block_background_color(block: &Block) -> String {
    let name = block.name.trim().to_lowercase();
    let key = if name.is_empty() { block.id.clone() } else { name };
    let hash = key.encode_utf16().fold(0u32, |hash, unit| hash.wrapping_mul(31).wrapping_add(u32::from(unit)));
    format!("hsl({} 78% 88%)", hash % 360)
}

fn to_base36(mut n: u64) -> String {
    let mut digits = Vec::new();
    loop {
        let d = (n % 36) as u8;
        digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.reverse();
    String::from_utf8(digits).expect("ascii")
}

// ---------------------------------------------------------------------------------------
// ScheduleBlock
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct ScheduleBlockProps {
    pub active_day_edge: Option<DayEdge>,
    pub active_resize_edge: Option<ResizeEdge>,
    pub block: Block,
    pub is_dragging: bool,
    pub is_draft: bool,
    pub is_horizontal_resizing: bool,
    pub is_resizing: bool,
    pub is_selected: bool,
    /// Blocks pushed aside by a drag slide to their preview position.
    pub should_animate_layout: bool,
    pub grid: Option<Handle<ScheduleGrid>>,
}

/// One block: its own editing state and name, reporting intents to the grid.
pub struct ScheduleBlock {
    editing: bool,
    name: String,
    last_committed_name: String,
    block_el: Option<NodeHandle>,
    input_el: Option<NodeHandle>,
}

impl ScheduleBlock {
    fn start_editing(&mut self, h: &Handle<Self>) {
        self.editing = true;
        h.update();
        h.queue_task(|this, _| {
            if let Some(input) = &this.input_el {
                dom_util::focus_without_scroll(input);
                input.select();
            }
        });
    }

    fn commit(&mut self, props: &ScheduleBlockProps) {
        let trimmed = self.name.trim().to_string();
        if !props.is_draft && trimmed == self.last_committed_name {
            return;
        }
        self.last_committed_name = trimmed.clone();
        let id = props.block.id.clone();
        with_grid(props, move |g, gh| g.commit_block(gh, &id, &trimmed));
    }

    fn finish_editing(&mut self, h: &Handle<Self>, focus_block: bool) {
        if !self.editing {
            return;
        }
        if let Some(props) = h.props() {
            self.commit(&props);
        }
        self.editing = false;
        h.update();
        if focus_block {
            if let Some(el) = &self.block_el {
                dom_util::focus_without_scroll(el);
            }
        }
    }

    fn cancel_editing(&mut self, h: &Handle<Self>) {
        self.name = self.last_committed_name.clone();
        if let Some(input) = &self.input_el {
            input.set_value(&self.last_committed_name);
        }
        self.editing = false;
        h.update();
        if let Some(el) = &self.block_el {
            dom_util::focus_without_scroll(el);
        }
    }
}

/// Run `f` on the grid once the current handler returns: the grid may be the code that
/// dispatched this event (its `blur()` and `focus()` calls fire block events), and it
/// re-renders this block.
fn with_grid(props: &ScheduleBlockProps, f: impl FnOnce(&mut ScheduleGrid, &Handle<ScheduleGrid>) + 'static) {
    if let Some(grid) = props.grid.clone() {
        grid.clone().spawn(async move {
            grid.with(f);
        });
    }
}

impl Component for ScheduleBlock {
    type Props = ScheduleBlockProps;

    fn setup(props: &ScheduleBlockProps, _: &Handle<Self>) -> Self {
        ScheduleBlock {
            editing: props.is_draft,
            name: props.block.name.clone(),
            last_committed_name: props.block.name.clone(),
            block_el: None,
            input_el: None,
        }
    }

    fn render(&self, props: &ScheduleBlockProps, h: &Handle<Self>) -> Node {
        let block = &props.block;
        // A draft starts in editing mode (`setup`); once committed, `is_draft` lags until the
        // grid re-renders, so only `self.editing` decides.
        let editing = self.editing;
        let label = if self.name.is_empty() { "Untitled".to_string() } else { self.name.clone() };

        let click = h.on_event(|_, h, event| {
            let Some(props) = h.props() else { return };
            let is_current = dom_util::target_is_current(event);
            if is_current && !event.prop_bool("shiftKey") {
                let id = props.block.id.clone();
                with_grid(&props, move |g, gh| g.select_block(gh, &id, false));
            }
            if is_current {
                dom_util::focus_current(event);
            }
        });
        let double_click = h.on_event(|this, h, event| {
            if dom_util::target_is_current(event) {
                event.prevent_default();
                this.start_editing(h);
            }
        });
        let focus = h.on_event(|_, h, event| {
            let Some(props) = h.props() else { return };
            if dom_util::target_is_current(event) {
                let id = props.block.id.clone();
                with_grid(&props, move |g, gh| g.focus_block(gh, &id));
            }
        });
        let key_down = h.on_event(|this, h, event| {
            let Some(props) = h.props() else { return };
            if !dom_util::target_is_current(event) {
                return;
            }
            let id = props.block.id.clone();
            match event.key().as_str() {
                "Escape" => {
                    event.prevent_default();
                    with_grid(&props, |g, gh| {
                        g.clear_block_focus_or_selection(gh);
                    });
                }
                "Enter" => {
                    event.prevent_default();
                    this.start_editing(h);
                }
                "Backspace" | "Delete" => {
                    event.prevent_default();
                    with_grid(&props, move |g, gh| g.delete_block(gh, &id));
                }
                _ => {}
            }
        });
        let pointer_down = h.on_event(move |_, h, event| {
            let Some(props) = h.props() else { return };
            if editing {
                return;
            }
            let pointer = Pointer::from_event(event);
            let id = props.block.id.clone();
            if pointer.shift {
                with_grid(&props, move |g, gh| g.select_block(gh, &id, true));
                return;
            }
            if dom_util::target_is_input(event) {
                return;
            }
            event.prevent_default();
            if dom_util::target_is_current(event) {
                dom_util::focus_current(event);
            }
            let block = props.block.clone();
            with_grid(&props, move |g, gh| g.block_pointer_down(gh, &block, pointer));
        });

        let input_events: Vec<Mixin> = vec![
            h.node_ref(|this, node| {
                this.input_el = Some(node.clone());
            }),
            on("input", h.on_event(|this, _, event| this.name = event.target_value())),
            on(
                "keydown",
                h.on_event(|this, h, event| match event.key().as_str() {
                    "Escape" => {
                        event.prevent_default();
                        let Some(props) = h.props() else { return };
                        if props.is_draft {
                            with_grid(&props, |g, gh| g.cancel_draft(gh));
                        } else {
                            this.cancel_editing(h);
                        }
                    }
                    "Enter" => {
                        event.prevent_default();
                        this.finish_editing(h, true);
                    }
                    _ => {}
                }),
            ),
            on("focus", h.on_event(|this, _, _| {
                if let Some(input) = &this.input_el {
                    input.select();
                }
            })),
            on("blur", h.on_event(|this, h, _| this.finish_editing(h, false))),
        ];
        if props.is_draft {
            h.queue_task(|this, _| {
                if let Some(input) = &this.input_el {
                    dom_util::focus_without_scroll(input);
                }
            });
        }

        let handles = (!editing).then(|| {
            rsx! {
                <>
                    {resize_handle(props, ResizeEdge::Start)}
                    {resize_handle(props, ResizeEdge::End)}
                    {day_handle(props, DayEdge::DayStart)}
                    {day_handle(props, DayEdge::DayEnd)}
                </>
            }
        });
        rsx! {
            <div data-dragging={props.is_dragging.then_some("true")} style={grid_area(block)} mix={[
                block_grid_item_style(),
                animate_layout(props.should_animate_layout.then(|| Layout::spring(SpringPreset::Snappy))),
            ]}>
                <div aria-label={label.clone()}
                    data-dragging={props.is_dragging.then_some("true")}
                    data-editing={editing.then_some("true")}
                    data-resizing={(props.is_resizing || props.is_horizontal_resizing).then_some("true")}
                    data-schedule-block="true"
                    data-schedule-block-id={block.id.clone()}
                    data-selected={props.is_selected.then_some("true")}
                    tabindex="0"
                    style={format!("background-color: {}", block_background_color(block))}
                    mix={[block_box_style(), h.node_ref(|this, node| this.block_el = Some(node.clone())),
                        on("click", click), on("dblclick", double_click), on("focus", focus), on("keydown", key_down), on("pointerdown", pointer_down)]}>
                    <input aria-label={if props.is_draft { "New block name".to_string() } else { format!("{label} name") }}
                        data-editable={editing.then_some("true")}
                        defaultValue={self.name.clone()}
                        readonly={!editing}
                        tabindex={if editing { "0" } else { "-1" }}
                        type="text"
                        mix={[vec![block_input_style()], input_events].concat()}/>
                    {handles}
                </div>
            </div>
        }
    }
}

fn resize_handle(props: &ScheduleBlockProps, edge: ResizeEdge) -> Node {
    let active = props.is_resizing && props.active_resize_edge == Some(edge);
    let grid = props.grid.clone();
    let block = props.block.clone();
    let start = rmx::EventHandler::new(move |event| {
        event.prevent_default();
        event.stop_propagation();
        let (block, pointer) = (block.clone(), Pointer::from_event(event));
        if let Some(grid) = &grid {
            let grid = grid.clone();
            grid.clone().spawn(async move {
                grid.with(|g, gh| g.start_resize(gh, &block, edge, pointer));
            });
        }
    });
    let (label, position) = match edge {
        ResizeEdge::Start => ("Start resize handle", "top: 0; transform: translateY(-4px);"),
        ResizeEdge::End => ("End resize handle", "bottom: 0; transform: translateY(4px);"),
    };
    rsx! {
        <div aria-label={label} class="resize-handle" data-active-resize-handle={active.then_some("true")}
            mix={[resize_handle_style(), css(position), on("pointerdown", start)]}></div>
    }
}

fn day_handle(props: &ScheduleBlockProps, edge: DayEdge) -> Node {
    let active = props.is_horizontal_resizing && props.active_day_edge == Some(edge);
    let grid = props.grid.clone();
    let block = props.block.clone();
    let start = rmx::EventHandler::new(move |event| {
        event.prevent_default();
        event.stop_propagation();
        let (block, pointer) = (block.clone(), Pointer::from_event(event));
        if let Some(grid) = &grid {
            let grid = grid.clone();
            grid.clone().spawn(async move {
                grid.with(|g, gh| g.start_horizontal_resize(gh, &block, edge, pointer));
            });
        }
    });
    let (label, position) = match edge {
        DayEdge::DayStart => ("First day resize handle", "left: 0; transform: translateX(-4px);"),
        DayEdge::DayEnd => ("Last day resize handle", "right: 0; transform: translateX(4px);"),
    };
    rsx! {
        <div aria-label={label} class="resize-handle" data-active-resize-handle={active.then_some("true")}
            mix={[day_handle_style(), css(position), on("pointerdown", start)]}></div>
    }
}

fn week_schedule_style() -> Mixin {
    css(r#"display: grid; grid-template-rows: 56px 32px minmax(0, 1fr); height: 100%; min-height: 0; overflow: hidden;
        &[data-schedule-dragging="true"], &[data-schedule-dragging="true"] * { cursor: grabbing !important; user-select: none; -webkit-user-select: none; }
        &[data-schedule-dragging="true"] .resize-handle { opacity: 0 !important; pointer-events: none; }
        &[data-schedule-resizing="vertical"], &[data-schedule-resizing="vertical"] * { cursor: ns-resize !important; user-select: none; -webkit-user-select: none; }
        &[data-schedule-resizing="horizontal"], &[data-schedule-resizing="horizontal"] * { cursor: ew-resize !important; user-select: none; -webkit-user-select: none; }"#)
}

fn block_grid_item_style() -> Mixin {
    css(r#"display: grid; min-height: 0; min-width: 0; pointer-events: none; position: relative; &[data-dragging="true"] { z-index: 2; }"#)
}

fn block_box_style() -> Mixin {
    css(&format!(
        r#"align-items: center; background-color: {lvl1}; border: 1px solid {border}; border-radius: {RADIUS}; box-shadow: 0 1px 1px rgb(0 0 0 / 0.05);
        color: #111111; cursor: grab; display: flex; font-size: {xs}; font-weight: 500; justify-content: center; margin: 2px 4px; overflow: hidden;
        padding: {pad}; pointer-events: auto; position: relative; text-align: center; touch-action: none;
        transition: background-color 120ms ease, border-color 120ms ease; user-select: none;
        &:not([data-dragging="true"]) {{ transition: background-color 120ms ease, border-color 120ms ease, translate 120ms ease; }}
        &:hover .resize-handle {{ opacity: 1; }}
        &[data-resizing="true"] .resize-handle {{ opacity: 1; }}
        &[data-editing="true"] {{ cursor: text; }}
        &:focus, &[data-selected="true"] {{ border-color: {FOCUS_RING}; outline: 2px solid {FOCUS_RING}; outline-offset: 0; }}
        &[data-dragging="true"] {{ background-color: {lvl2}; border-color: {FOCUS_RING}; cursor: grabbing; outline: 2px solid {FOCUS_RING}; outline-offset: 0; z-index: 2; }}
        &[data-dragging="true"] .resize-handle {{ opacity: 0; }}"#,
        lvl1 = surface::LVL1,
        lvl2 = surface::LVL2,
        border = border::DEFAULT,
        xs = font_size::XS,
        pad = space::XS,
    ))
}

fn resize_handle_style() -> Mixin {
    css(&format!(
        r#"cursor: ns-resize; height: 12px; left: {xs}; opacity: 0; position: absolute; right: {xs}; touch-action: none; z-index: 3;
        &::before {{ background-color: {FOCUS_RING}; border-radius: 999px; content: ""; height: 3px; left: 50%; position: absolute; top: 50%; transform: translate(-50%, -50%); width: 28px; }}
        &:hover {{ opacity: 1; }} &[data-active-resize-handle="true"] {{ opacity: 1; }}"#,
        xs = space::XS
    ))
}

fn day_handle_style() -> Mixin {
    css(&format!(
        r#"bottom: {sm}; cursor: ew-resize; opacity: 0; position: absolute; top: {sm}; touch-action: none; width: 12px; z-index: 3;
        &::before {{ background-color: {FOCUS_RING}; border-radius: 999px; content: ""; height: 20px; left: 50%; position: absolute; top: 50%; transform: translate(-50%, -50%); width: 3px; }}
        &:hover {{ opacity: 1; }} &[data-active-resize-handle="true"] {{ opacity: 1; }}"#,
        sm = space::SM
    ))
}

fn block_input_style() -> Mixin {
    css(&format!(
        r#"background-color: transparent; border: 0; box-sizing: border-box; color: inherit; cursor: inherit; font: inherit; font-weight: inherit;
        height: 100%; inset: 0; line-height: 1.2; outline: 0; padding: {xs}; pointer-events: none; position: absolute; text-align: center;
        user-select: none; -webkit-user-select: none; width: 100%;
        &[data-editable="true"] {{ pointer-events: auto; user-select: text; -webkit-user-select: text; }}"#,
        xs = space::XS
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_time_slots() {
        assert_eq!(format_time_slot(0), "12:00am");
        assert_eq!(format_time_slot(390), "6:30am");
        assert_eq!(format_time_slot(12 * 60 + 15), "12:15pm");
        assert_eq!(format_time_slot(23 * 60 + 45), "11:45pm");
    }

    #[test]
    fn shift_selection_takes_the_run_between_anchor_and_target() {
        let b = |id: &str, day, start| Block { id: id.into(), day_of_week: day, start_minute: start, end_minute: start + 15, ..Default::default() };
        let blocks = [b("a", 0, 0), b("b", 0, 15), b("c", 0, 30), b("d", 1, 0)];
        assert_eq!(adjacent_selection(&blocks, "c", "a"), ["a", "b", "c"]);
        assert_eq!(adjacent_selection(&blocks, "a", "d"), ["d"]);
    }

    #[test]
    fn colors_are_stable_per_name() {
        let b = |name: &str| Block { id: "x".into(), name: name.into(), ..Default::default() };
        assert_eq!(block_background_color(&b("Read")), block_background_color(&b(" read ")));
        assert_eq!(to_base36(35), "z");
    }
}
