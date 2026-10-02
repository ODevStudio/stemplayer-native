use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use slint::{ComponentHandle, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
use std::rc::Rc;

slint::slint! {
    import { Palette } from "std-widgets.slint";
    import { PlaylistPanel, PlaylistRow } from "../ui/chrome.slint";

    export component PlaylistScrollWindow inherits Window {
        preferred-width: 320px;
        preferred-height: 220px;
        in property <[PlaylistRow]> items;
        in-out property <int> selected-index: -1;
        in-out property <bool> expanded: true;
        out property <color> selected-color: Palette.selection-background;

        if root.items.length > 0: PlaylistPanel {
            width: parent.width;
            height: parent.height;
            items: root.items;
            selected-index <=> root.selected-index;
            expanded <=> root.expanded;
        }
    }
}

struct TestPlatform(Rc<MinimalSoftwareWindow>);

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

#[test]
fn selected_row_is_visible_after_restore_import_resize_and_expansion() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(Rc::clone(&window)))).unwrap();
    let app = PlaylistScrollWindow::new().unwrap();
    app.set_items(rows(100, 80));
    app.set_selected_index(80);
    app.show().unwrap();
    let restored_row_top = selected_row_top(&app, &window, 320, 220).unwrap();

    app.set_items(rows(100, 79));
    app.set_selected_index(79);
    assert_eq!(
        selected_row_top(&app, &window, 320, 220),
        Some(restored_row_top - 28)
    );

    app.set_items(rows(120, 100));
    app.set_selected_index(100);
    assert!(selected_row_top(&app, &window, 320, 220).is_some());
    assert!(selected_row_top(&app, &window, 180, 100).is_some());

    app.set_expanded(false);
    assert!(selected_row_top(&app, &window, 320, 220).is_none());
    app.set_expanded(true);
    assert!(selected_row_top(&app, &window, 320, 220).is_some());

    app.set_items(rows(120, 0));
    app.set_selected_index(0);
    assert!(selected_row_top(&app, &window, 320, 220).is_some());
}

fn rows(count: usize, selected: usize) -> ModelRc<PlaylistRow> {
    ModelRc::new(VecModel::from(
        (0..count)
            .map(|index| PlaylistRow {
                label: format!("Track {index}").into(),
                selected: index == selected,
            })
            .collect::<Vec<_>>(),
    ))
}

fn selected_row_top(
    app: &PlaylistScrollWindow,
    window: &MinimalSoftwareWindow,
    width: u32,
    height: u32,
) -> Option<usize> {
    window.set_size(PhysicalSize::new(width, height));
    app.window().request_redraw();
    let mut pixels = vec![Rgb8Pixel::default(); (width * height) as usize];
    for _ in 0..2 {
        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    let color = app.get_selected_color().to_argb_u8();
    let selected = Rgb8Pixel::new(color.red, color.green, color.blue);
    let selected_rows = pixels
        .chunks_exact(width as usize)
        .enumerate()
        .skip(34)
        .filter(|(_, row)| row[5] == selected)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert!(
        selected_rows.is_empty() || selected_rows.len() >= 27,
        "Selected row is clipped: {selected_rows:?}"
    );
    (selected_rows.len() >= 27).then(|| selected_rows[0])
}
