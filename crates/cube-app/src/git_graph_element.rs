//! 泳道图绘制元素。
//!
//! GPUI 没有现成的「画曲线」元素，这里实现一个最小的 `Element`：拿自己的
//! bounds，按行高把每个提交的圆点和车道连线画出来。布局由
//! [`cube_core::git_graph`] 算好，这里只负责照坐标落笔。

use cube_core::git_graph::{GitGraphLayout, GitRowLayout};
use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, Hsla, IntoElement, LayoutId, PathBuilder,
    Pixels, Point, Style, Window, point, px,
};

/// 侧栏里最多同时显示几条车道。超出的车道不画，避免把提交文字挤没。
const MAX_VISIBLE_LANES: usize = 8;

/// 每条车道的可用颜色，按索引取用。
fn lane_color(index: usize, theme: &gpui_component::Theme) -> Hsla {
    const PALETTE_SIZE: usize = 6;
    match index % PALETTE_SIZE {
        0 => theme.primary,
        1 => theme.success,
        2 => theme.warning,
        3 => theme.danger,
        4 => theme.link,
        _ => theme.info,
    }
}

pub struct GitGraphElement {
    layout: GitGraphLayout,
    row_height: f32,
    lane_width: f32,
    /// 每行圆点相对本行顶部的偏移，一般取 row_height / 2。
    dot_radius: f32,
    line_width: f32,
    theme: gpui_component::Theme,
}

impl GitGraphElement {
    pub fn new(
        layout: GitGraphLayout,
        row_height: f32,
        lane_width: f32,
        theme: gpui_component::Theme,
    ) -> Self {
        Self {
            layout,
            row_height,
            lane_width,
            dot_radius: 3.5,
            line_width: 1.5,
            theme,
        }
    }

    fn lane_x(&self, lane: usize) -> f32 {
        // 车道数被 max_lanes 限制过，这里直接按宽度算即可。
        self.lane_width * 0.5 + lane as f32 * self.lane_width
    }
}

impl IntoElement for GitGraphElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for GitGraphElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // 侧栏很窄，车道太多时截断到上限，避免把提交信息挤到看不见。
        let lanes = self.layout.max_lanes.clamp(1, MAX_VISIBLE_LANES);
        let mut style = Style::default();
        let height = px(self.row_height * self.layout.rows.len() as f32);
        style.size.width = px(self.lane_width * lanes as f32).into();
        // 高度必须精确：圆点位置是按 row_height 递推的，一旦被父容器拉伸
        // 就会和右侧文字逐行错开。
        style.size.height = height.into();
        style.min_size.height = height.into();
        style.max_size.height = height.into();
        style.flex_shrink = 0.0;
        // 让这一列占据整块行高，与提交列表对齐。
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let origin = bounds.origin;
        // 行高取固定值：request_layout 已用 min/max height 锁死元素高度，
        // 这里再按 bounds 重算会和布局层打架。右侧列表也用同一个常量，
        // 两边才能逐行对齐。
        let row_height = self.row_height;
        for (index, row) in self.layout.rows.iter().enumerate() {
            let top = origin.y + px(index as f32 * row_height);
            let center_y = top + px(row_height * 0.5);
            let next_center_y = center_y + px(row_height);
            self.paint_row(window, origin, row, top, center_y, next_center_y);
        }
    }
}

impl GitGraphElement {
    fn paint_row(
        &self,
        window: &mut Window,
        origin: Point<Pixels>,
        row: &GitRowLayout,
        row_top: Pixels,
        center_y: Pixels,
        next_center_y: Pixels,
    ) {
        let row_height = self.row_height;
        let dot_x = origin.x + px(self.lane_x(row.lane));
        let dot_color = lane_color(row.color, &self.theme);

        // 贯穿本行的其它车道：从行顶画到行底，和上下行严丝合缝。
        let row_bottom = row_top + px(row_height);
        for (lane, color) in &row.pass_through {
            if *lane >= MAX_VISIBLE_LANES {
                continue;
            }
            let x = origin.x + px(self.lane_x(*lane));
            let mut line = PathBuilder::stroke(px(self.line_width));
            line.move_to(point(x, row_top));
            line.line_to(point(x, row_bottom));
            if let Ok(path) = line.build() {
                window.paint_path(path, lane_color(*color, &self.theme));
            }
        }

        // 车道超出可视范围时，圆点也不画（只保留贯穿线的截断处）。
        if row.lane >= MAX_VISIBLE_LANES {
            return;
        }

        // 圆点用多边形近似，不用 arc_to。
        //
        // arc_to 在弧恰好是 180°（两端点互为直径）时数值退化，lyon 会丢掉
        // 整段，表现就是圆点偶尔缺半个——正是之前看到的「下半圆绘制不出来」。
        // 细分成 16 段的正多边形，半径 3.5px，肉眼看不出棱角。
        let mut dot = PathBuilder::fill();
        let r = self.dot_radius;
        const SEGMENTS: usize = 16;
        for index in 0..SEGMENTS {
            let angle = index as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let x = dot_x + px(r * angle.cos());
            let y = center_y + px(r * angle.sin());
            if index == 0 {
                dot.move_to(point(x, y));
            } else {
                dot.line_to(point(x, y));
            }
        }
        dot.close();
        if let Ok(path) = dot.build() {
            window.paint_path(path, dot_color);
        }

        // 连向下一行：同车道画竖线，换车道画曲线。
        for edge in &row.edges {
            let from_x = dot_x;
            let to_x = origin.x + px(self.lane_x(edge.to_lane));
            let color = lane_color(edge.color, &self.theme);
            if (pixels_to_f32(from_x) - pixels_to_f32(to_x)).abs() < 0.5 {
                // 同车道：从圆点画到下一行中心。上半段（行顶到圆点）由上一行
                // 的线负责，重复绘制会让线变粗、颜色叠加。
                let mut line = PathBuilder::stroke(px(self.line_width));
                line.move_to(point(from_x, center_y));
                line.line_to(point(to_x, next_center_y));
                if let Ok(path) = line.build() {
                    window.paint_path(path, color);
                }
            } else {
                // 换道用三次贝塞尔，弧度收在上半段，视觉上像 VSCode 的汇入。
                // 控制点放在水平中点和垂直中点：线先横向拉开，再竖直落下，
                // 弧线明显且不贴着圆点，看起来不会像"没有线"。
                let mut curve = PathBuilder::stroke(px(self.line_width));
                let mid_x = px((pixels_to_f32(from_x) + pixels_to_f32(to_x)) * 0.5);
                curve.move_to(point(from_x, center_y));
                curve.cubic_bezier_to(
                    point(to_x, next_center_y),
                    point(mid_x, center_y),
                    point(mid_x, next_center_y),
                );
                if let Ok(path) = curve.build() {
                    window.paint_path(path, color);
                }
            }
        }
    }
}

fn pixels_to_f32(value: Pixels) -> f32 {
    value.as_f32()
}
