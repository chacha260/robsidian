//! マウスホイールのスクロールを、遅れなく反映させる。
//!
//! egui は、行単位・ページ単位のホイール入力（ノッチ付きのマウスホイール）を、
//! 0.1秒で9割進むようにならしてから適用する。なめらかに見える反面、回した瞬間には動かず、遅れて感じる。
//! 一方で、ピクセル単位の小さな入力（トラックパッド）はならさずにすぐ適用する。
//! そこで入力を egui に渡す前に、行単位・ページ単位の入力をピクセル単位に換算し、
//! ならされない大きさ（8pt未満）に分けて渡す。動く量は変わらない。

use eframe::egui::{Event, MouseWheelUnit, RawInput, Vec2};

/// egui がこれ未満のピクセル単位の入力を「なめらかな入力」とみなしてすぐ適用する大きさ（の少し手前）
const PIECE: f32 = 7.5;

/// egui の既定の、1行あたりのスクロール量（`InputOptions::line_scroll_speed`）
const LINE: f32 = 40.0;

/// 行単位・ページ単位のホイール入力を、すぐ適用される小さなピクセル単位の入力に置き換える。
pub fn make_immediate(raw: &mut RawInput) {
    if !raw.events.iter().any(is_coarse_wheel) {
        return;
    }
    let page = raw.screen_rect.map_or(600.0, |r| r.height());
    let events = std::mem::take(&mut raw.events);
    for event in events {
        let Event::MouseWheel {
            unit,
            delta,
            phase,
            source,
            modifiers,
        } = event
        else {
            raw.events.push(event);
            continue;
        };
        let points = match unit {
            MouseWheelUnit::Point => {
                raw.events.push(event);
                continue;
            }
            MouseWheelUnit::Line => delta * LINE,
            MouseWheelUnit::Page => delta * page,
        };
        let pieces = (points.length() / PIECE).ceil().max(1.0);
        let step: Vec2 = points / pieces;
        for _ in 0..pieces as usize {
            raw.events.push(Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: step,
                phase,
                source,
                modifiers,
            });
        }
    }
}

fn is_coarse_wheel(e: &Event) -> bool {
    matches!(
        e,
        Event::MouseWheel {
            unit: MouseWheelUnit::Line | MouseWheelUnit::Page,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Modifiers, MouseWheelSource, Rect, TouchPhase};

    fn wheel(unit: MouseWheelUnit, delta: Vec2) -> Event {
        Event::MouseWheel {
            unit,
            delta,
            phase: TouchPhase::Move,
            source: MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        }
    }

    fn total(events: &[Event]) -> Vec2 {
        events
            .iter()
            .map(|e| match e {
                Event::MouseWheel { delta, .. } => *delta,
                _ => Vec2::ZERO,
            })
            .fold(Vec2::ZERO, |a, b| a + b)
    }

    #[test]
    fn splits_line_and_page_scrolls_into_small_point_scrolls() {
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Default::default(),
                Vec2::new(800.0, 600.0),
            )),
            events: vec![
                Event::PointerGone,
                wheel(MouseWheelUnit::Line, Vec2::new(0.0, -1.0)),
                wheel(MouseWheelUnit::Page, Vec2::new(0.0, 1.0)),
                wheel(MouseWheelUnit::Point, Vec2::new(0.0, 3.0)),
            ],
            ..Default::default()
        };
        make_immediate(&mut raw);
        // 動く量は変わらない（1行 = 40pt、1ページ = 画面の高さ）
        assert_eq!(total(&raw.events), Vec2::new(0.0, -40.0 + 600.0 + 3.0));
        // すべてピクセル単位で、どれも egui がならさない大きさ
        for e in &raw.events {
            if let Event::MouseWheel { unit, delta, .. } = e {
                assert_eq!(*unit, MouseWheelUnit::Point);
                assert!(delta.length() < 8.0, "{delta:?}");
            }
        }
        // 他の入力と順番はそのまま
        assert!(matches!(raw.events[0], Event::PointerGone));
        assert!(
            matches!(raw.events.last(), Some(Event::MouseWheel { delta, .. }) if delta.y == 3.0)
        );
    }

    #[test]
    fn leaves_input_without_coarse_wheel_alone() {
        let mut raw = RawInput {
            events: vec![wheel(MouseWheelUnit::Point, Vec2::new(0.0, 20.0))],
            ..Default::default()
        };
        make_immediate(&mut raw);
        assert_eq!(raw.events.len(), 1);
    }
}
