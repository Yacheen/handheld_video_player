use embedded_graphics::{
  pixelcolor::{BinaryColor, Rgb565}, prelude::*, primitives::{Circle, Line, Polyline, PrimitiveStyle, Rectangle}
};

use crate::FramebufferDisplay;
pub fn draw_clock(fb: &mut [u8], width: usize, height: usize, top_left: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let style = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    // // Rough octagon for clock circle outline
    // let outline = [
    //     top_left + Point::new(4, 0),
    //     top_left + Point::new(11, 0),
    //     top_left + Point::new(15, 4),
    //     top_left + Point::new(15, 11),
    //     top_left + Point::new(11, 15),
    //     top_left + Point::new(4, 15),
    //     top_left + Point::new(0, 11),
    //     top_left + Point::new(0, 4),
    //     top_left + Point::new(4, 0), // close
    // ];
    // Polyline::new(&outline)
    //     .into_styled(style)
    //     .draw(&mut display)
    //     .unwrap();
    //
    // let center = top_left + Point::new(8, 8);
    //
    // // Hour hand
    // Line::new(center, center + Point::new(0, -3))
    //     .into_styled(style)
    //     .draw(&mut display)
    //     .unwrap();
    //
    // // Minute hand
    // Line::new(center, center + Point::new(3, 2))
    //     .into_styled(style)
    //     .draw(&mut display)
    //     .unwrap();

    
    
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
        top_left + Point::new(3, 6),    // close shape
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
        top_left + Point::new(6, 18), // close shape
    ];

    Polyline::new(&outline)
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
