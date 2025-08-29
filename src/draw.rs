use embedded_graphics::{
  pixelcolor::{BinaryColor, Rgb565}, prelude::*, primitives::{Circle, CornerRadii, Line, Polyline, PrimitiveStyle, Rectangle, RoundedRectangle, Triangle}
};
// constants
pub const TOP_NAV_BG_COORDS: Point = Point::new(10, 10);
pub const TOP_NAV_FILE_INDEX_COORDS: Point = Point::new(46, 18);
pub const TOP_NAV_WEATHER_TEXT_COORDS: Point = Point::new(164, 16);
pub const TOP_NAV_CLOCK_TEXT_COORDS: Point = Point::new(244, 16);
pub const TOP_CAROUSEL_BG_COORDS: Point = Point::new(40, 90);
pub const MIDDLE_CAROUSEL_BG_COORDS: Point = Point::new(40, 140);
pub const BOTTOM_CAROUSEL_BG_COORDS: Point = Point::new(40, 190);

pub const TOP_CAROUSEL_TXT_COORDS: Point = Point::new(50, 106);
pub const MIDDLE_CAROUSEL_TXT_COORDS: Point = Point::new(50, 156);
pub const BOTTOM_CAROUSEL_TXT_COORDS: Point = Point::new(50, 206);

use crate::FramebufferDisplay;
pub fn draw_clock(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    // Use a radius of 8 to allow 1px stroke padding around the circle
    let radius = 8;
    let center = top_left + Point::new(10, 10); // perfectly centered in 20x20

    // Draw the clock face (circle)
    Circle::new(center - Size::new(radius, radius), radius * 2)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    // Hour hand (upward)
    Line::new(center, center + Point::new(0, -4))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    // Minute hand (rightward)
    Line::new(center, center + Point::new(5, 0))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_folder(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    // Folder outline (with tab)
    let outline = [
        top_left + Point::new(3, 6),    // bottom of tab
        top_left + Point::new(3, 4),    // tab left
        top_left + Point::new(10, 4),   // tab top
        top_left + Point::new(12, 6),   // tab right corner down

        top_left + Point::new(24, 6),   // right top of folder
        top_left + Point::new(24, 18),  // bottom right
        top_left + Point::new(3, 18),   // bottom left
        top_left + Point::new(3, 6),    
    ];
    Polyline::new(&outline)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_cloud(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);
    
    let outline = [
        top_left + Point::new(6, 18),
        top_left + Point::new(4, 17),
        top_left + Point::new(2, 15),
        top_left + Point::new(1, 12),
        top_left + Point::new(2, 9),
        top_left + Point::new(5, 7),
        top_left + Point::new(8, 6),
        top_left + Point::new(9, 4),
        top_left + Point::new(12, 2),
        top_left + Point::new(15, 1),
        top_left + Point::new(18, 2),
        top_left + Point::new(20, 3),
        top_left + Point::new(21, 5),
        top_left + Point::new(23, 6),
        top_left + Point::new(25, 8),
        top_left + Point::new(27, 11),
        top_left + Point::new(28, 14),
        top_left + Point::new(27, 16),
        top_left + Point::new(25, 17),
        top_left + Point::new(22, 18),
        top_left + Point::new(6, 18), 
    ];

    Polyline::new(&outline)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_camera(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);
    
    // 1. Camera Body (Rounded Rectangle)
    let body_top_left = top_left + Point::new(4, 6);
    let body_size = Size::new(22, 10); // Size 22x10 (width x height)
    let corner_radius = Size::new(3, 3);
    let body = RoundedRectangle::with_equal_corners(Rectangle::new(body_top_left, body_size), corner_radius);
    body.into_styled(style)
        .draw(&mut display)
        .unwrap();

    // 2. Lens (Triangle facing right)
    let lens = Triangle::new(
        top_left + Point::new(26, 10), // Top point of the triangle (lens)
        top_left + Point::new(26, 14), // Bottom-left point of the triangle
        top_left + Point::new(30, 12), // Bottom-right point of the triangle
    );
    lens.into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_file(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    // Define the rectangle
    let rectangle_points = [
        top_left + Point::new(2, 2), top_left + Point::new(18, 2), top_left + Point::new(18, 18),
        top_left + Point::new(2, 18),
    ];

    // Define the folded corner
    let folded_corner_points = [
        top_left + Point::new(14, 2), top_left + Point::new(16, 4),
    ];

    // Draw the rectangle
    Polyline::new(&rectangle_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    // Draw the folded corner
    Polyline::new(&folded_corner_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_question_mark(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);
    
    // Define the question mark
    let question_mark_points = [
        top_left + Point::new(10, 5), top_left + Point::new(10, 10), top_left + Point::new(10, 12),
        top_left + Point::new(10, 13),
    ];

    // Draw the question mark
    Polyline::new(&question_mark_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_music_note(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    let points = [
        top_left + Point::new(5, 5),
        top_left + Point::new(10, 5),
        top_left + Point::new(10, 15),
        top_left + Point::new(15, 15),
        top_left + Point::new(15, 10),
        top_left + Point::new(20, 10),
        top_left + Point::new(20, 5),
    ];
    Polyline::new(&points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_playing(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    // Define the circle
    let circle_points = [
        top_left + Point::new(10, 1), top_left + Point::new(15, 3), top_left + Point::new(18, 7),
        top_left + Point::new(18, 13), top_left + Point::new(15, 17), top_left + Point::new(10, 19),
        top_left + Point::new(5, 17), top_left + Point::new(2, 13), top_left + Point::new(2, 7),
        top_left + Point::new(5, 3),
    ];

    // Define the play triangle
    let triangle_points = [
        top_left + Point::new(7, 5), top_left + Point::new(7, 15), top_left + Point::new(13, 10),
    ];

    // Draw the circle
    Polyline::new(&circle_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    // Draw the play triangle
    Polyline::new(&triangle_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_paused(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    
    // Define the circle
    let circle_points = [
        top_left + Point::new(10, 1), top_left + Point::new(15, 3), top_left + Point::new(18, 7),
        top_left + Point::new(18, 13), top_left + Point::new(15, 17), top_left + Point::new(10, 19),
        top_left + Point::new(5, 17), top_left + Point::new(2, 13), top_left + Point::new(2, 7),
        top_left + Point::new(5, 3),
    ];

    // Define the pause bars
    let pause_bar1_points = [
        top_left + Point::new(6, 5), top_left + Point::new(6, 15),
    ];
    let pause_bar2_points = [
        top_left + Point::new(12, 5), top_left + Point::new(12, 15),
    ];

    // Draw the circle
    Polyline::new(&circle_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    // Draw the pause bars
    Polyline::new(&pause_bar1_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
    Polyline::new(&pause_bar2_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
pub fn draw_volume(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };    
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    
    // Define the speaker
    let speaker_points = [
        top_left + Point::new(5, 5), top_left + Point::new(5, 15), top_left + Point::new(9, 15),
        top_left + Point::new(9, 5),
    ];

    // Define the sound waves
    let wave1_points = [
        top_left + Point::new(10, 10), top_left + Point::new(13, 7),
    ];
    let wave2_points = [
        top_left + Point::new(10, 12), top_left + Point::new(14, 6),
    ];
    let wave3_points = [
        top_left + Point::new(10, 14), top_left + Point::new(15, 5),
    ];

    // Draw the speaker
    Polyline::new(&speaker_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();

    // Draw the sound waves
    Polyline::new(&wave1_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
    Polyline::new(&wave2_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
    Polyline::new(&wave3_points)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
    
