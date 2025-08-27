use chrono::{DateTime, Local};
use embedded_graphics::{
    mono_font::{ascii::{FONT_6X10, FONT_8X13}, MonoTextStyle, MonoTextStyleBuilder}, pixelcolor::{BinaryColor, Rgb565}, prelude::*, primitives::{PrimitiveStyleBuilder, Rectangle, RoundedRectangle}, text::{Baseline, Text}
};
use embedded_hal::digital::{InputPin, OutputPin};
use linux_embedded_hal::gpio_cdev::{Chip, EventRequestFlags, EventType, LineRequestFlags, AsyncLineEventHandle};
use linux_embedded_hal::i2cdev::core::I2CDevice;
use linux_embedded_hal::{ CdevPin };
use std::{collections::HashMap, io::prelude::*, path::PathBuf, sync::{Arc, Mutex}};
use std::fs::File;
use std::thread;
use linuxfb::Framebuffer;
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
use tokio::{sync::mpsc, time::{sleep, Duration, Instant}};
use futures::StreamExt;
use debouncr::{debounce_4, Debouncer, Edge, Repeat4};
use std::process::Command;

const WIDTH: usize = 320;
const HEIGHT: usize = 240;
// const FRAME_SIZE: usize = WIDTH * HEIGHT * 2;
const SSD1306_SLAVE_ADDR: u16 = 0x3c;

// mods
mod draw;

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
enum ButtonEvent {
    Up,
    Down,
    Select,
    Escape,
}
enum DisplayState {
    Navigating,
    PlayingSomething,
    ConfirmingMediaSelection,
    ConfirmingMediaExit,
    UnrecoverableError,
    ErrorMessage,
}
struct State {
    current_state: DisplayState,
    previous_state: DisplayState,
    nav_state: NavigatingData,
    video_state: PlayingSomethingData,
    modal_state: (String, bool),
    error_state: String,
}
struct NavigatingData {
    current_dir: PathBuf,
    current_index: usize,
    file_count: usize,
}
struct PlayingSomethingData {
    paused: bool,
    timestamp: u64,
    volume: u8,
}
// when navigating:
// show directory on top, file on bottom small screens

// when pressing esc, show exitconfirmation no on top, yes on bottom

// show ok button on bottom screen to exit error

// show shut down button on bottom screen during unrecoverable error

// show what's currently playing on first samll screen
// show timestamp & volume on 2nd screen (and what else?)

// what do I need
// store timestamp of where ur currently at in video in terms of bytes
// esc - 
// if Navigating, go outside dir or show error msg when going too high.
// if PlayingSomething - set state to ConfirmingMediaExit and pause video, and previous_state
// if ConfirmingMediaSelection, set state to Navigating, and previous_state
// 
// either cd out of dir or bringup "are u sure u wanna stop watching" modal
//
// select - 
// if navigating state, go inside dir.
// else if PlayingSomething, pause or continue video based on current state of PlayingSomething
// else if ConfirmingSelectingMedia, choose the option the bool is currently set to (0 = no, 1 = yes)
// else if ConfirmingExitPlaying, choose the option the bool is currently set to (0 = no, 1 = yes)
//
// up - 
// if navigating, goto previous file,
// if playingSomething, increase volume
// if ConfirmingSelectingMedia, set bool to no (0)
// if ConfirmingExitPlaying, set bool to no (0)
//
// down - 
// if Navigating, goto next file
// if PlayingSomething, turn volume down
// if ConfirmingSelectingMedia, set bool to yes (1)
// if ConfirmingMediaExit, set bool to  yes (1)

#[tokio::main]
async fn main() -> ! {
    // set current directory to home
    std::env::set_current_dir("/home/yassin/").unwrap();
    let output = Command::new("./setup_gpios.sh")
        .output()
        .expect("Failed to execute setup gpio pullup inputs")
    ;
    thread::sleep(Duration::from_secs(2));

    // gpio setup
    let chip_path = "/dev/gpiochip0";
    let mut chip = Chip::new(chip_path).unwrap();

    // i2c
    let i2c_screen1_dev = linux_embedded_hal::I2cdev::new("/dev/i2c-1").unwrap();
    let i2c_screen2_dev = linux_embedded_hal::I2cdev::new("/dev/i2c-2").unwrap();

    // i2c screen1
    let i2c_screen1_interface = I2CDisplayInterface::new(i2c_screen1_dev);
    let mut i2c_screen1_display = Ssd1306::new(i2c_screen1_interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    i2c_screen1_display.init().unwrap();
    i2c_screen1_display.clear_buffer();
    i2c_screen1_display.flush().unwrap();

    // i2c screen2
    let i2c_screen2_interface = I2CDisplayInterface::new(i2c_screen2_dev);
    let mut i2c_screen2_display = Ssd1306::new(i2c_screen2_interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    i2c_screen2_display.init().unwrap();
    i2c_screen2_display.clear_buffer();
    i2c_screen2_display.flush().unwrap();

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_8X13)
        .text_color(BinaryColor::On)
        .build();
    
    Text::with_baseline("Hello, Yassin.", Point::zero(), text_style, Baseline::Top)
        .draw(&mut i2c_screen2_display)
        .unwrap();
    i2c_screen2_display.flush().unwrap();

    let mut backlight = CdevPin::new(chip.get_line(5).unwrap().request(LineRequestFlags::OUTPUT, 1, "bl").unwrap()).unwrap();
    backlight.set_high().unwrap();

    // STATE INITIALIZATION----------------------------------------------------------------------------------------------------
    let current_dir = std::env::current_dir().unwrap();
    let current_dir_as_str= current_dir
        .iter()
        .last()
        .unwrap()
        .to_str()
        .unwrap();

    let file_count = std::fs::read_dir(std::env::current_dir().unwrap().as_path()).unwrap().count();
    // this'll give you: 2069-01-24 13:17:44.609871 UTC or something.
    let current_local_time: DateTime<Local> = Local::now();
    let formatted_local_time = current_local_time.format("%H:%M");
    println!("file count!: {}", file_count);
    println!("formatted local time: {:?}", formatted_local_time);


    let mut state = State {
        current_state: DisplayState::Navigating,
        previous_state: DisplayState::Navigating,
        nav_state: NavigatingData {
            current_dir: current_dir.clone(),
            file_count: file_count,
            current_index: 0,
        },
        video_state: PlayingSomethingData {
            paused: true,
            volume: 0,
            timestamp: 0,
        },
        modal_state: (String::new(), false),
        error_state: String::new(),
    };

    // buttons channels and tasks-------------------------------------------------------------
    // btn channel
    let (btn_tx, mut btn_rx) = mpsc::channel(32);
    // draw channel
    let (draw_tx, mut draw_rx) = mpsc::channel::<DrawCommand>(32);
    // video/music task command channel (pause, resume, stop)
    let (media_tx, mut media_rx) = mpsc::channel::<ControlCommand>(32);

    // select
    tokio::spawn(button_task(chip_path, 19, btn_tx.clone(), ButtonEvent::Select));
    // escape
    tokio::spawn(button_task(chip_path, 26, btn_tx.clone(), ButtonEvent::Escape));
    // up
    tokio::spawn(button_task(chip_path, 13, btn_tx.clone(), ButtonEvent::Up));
    // down
    tokio::spawn(button_task(chip_path, 6, btn_tx.clone(), ButtonEvent::Down));

    // draw task - will draw whatever until end of program
    tokio::spawn(start_drawing_task(draw_rx));

    // wait for start_drawing_task to be ready
    std::thread::sleep(Duration::from_millis(500));

    i2c_screen1_display.clear_buffer();
    i2c_screen1_display.flush().unwrap();
    Text::with_baseline(current_dir_as_str, Point::zero(), text_style, Baseline::Top)
        .draw(&mut i2c_screen1_display)
        .unwrap();
    i2c_screen1_display.flush().unwrap();

    // initialize nav ui. the rest will be handled below on button presses based on states
    let mut drawings = Vec::new();
    // top meta
    drawings.push(
        RoundedRectangle::with_equal_corners(
            Rectangle::new(draw::TOP_NAV_BG_COORDS, Size::new(300, 40)),
            Size::new(10, 10),
        )
    );
    // selection carousel
    {
        drawings.append(
            &mut vec![
                RoundedRectangle::with_equal_corners(
                    Rectangle::new(draw::TOP_CAROUSEL_BG_COORDS, Size::new(270, 40)),
                    Size::new(10, 10),
                ),
                RoundedRectangle::with_equal_corners(
                    Rectangle::new(draw::MIDDLE_CAROUSEL_BG_COORDS, Size::new(270, 40)),
                    Size::new(10, 10),
                ),
                RoundedRectangle::with_equal_corners(
                    Rectangle::new(draw::BOTTOM_CAROUSEL_BG_COORDS, Size::new(270, 40)),
                    Size::new(10, 10),
                ),
            ]
        );
    }



    // Static drawings
    draw_tx.send(DrawCommand::ClearScreen).await.unwrap();
    std::thread::sleep(Duration::from_millis(500));

    draw_tx.send(DrawCommand::NavigatingBackground { drawings }).await.unwrap();

    // Dynamic drawings (state)

    // current path, file_count, and current file navigated on (index of files of dir)
    draw_tx.send(DrawCommand::Text { content: format_dir(current_dir.to_owned()).to_str().unwrap().to_owned(), position: Point::new(20, 36) }).await.unwrap();
    draw_tx.send(DrawCommand::Text { content: format!("1/{}", file_count), position: Point::new(46, 18) }).await.unwrap();
    // if index = 0, set top to nothing, set middle to 0, and bottom to 1
    // if index = 1, set top to 0, middle to 1, bottom to 2
    // if index = 2, set top to 1, middle to 2, bottom to 3
    // ...
    // if index = 18, set top to 17, middle to 18, bottom to nothing

    // draw indexes 0 and 1 to middle and bottom.
    for (index, entry) in std::fs::read_dir(current_dir.to_owned()).unwrap().enumerate() {
        let dir = entry.unwrap();
        if index == 0 {
            draw_tx.send(DrawCommand::Text { 
                content: dir.file_name().to_str().unwrap().to_owned(), position: Point::new(50, 156) 
            }).await.unwrap();

        }
        else if index == 1 {
            draw_tx.send(DrawCommand::Text { 
                content: dir.file_name().to_str().unwrap().to_owned(), position: Point::new(50, 206) 
            }).await.unwrap();
        }
    }

    // texts
    // current, previous, and next indexes's data.
    // draw icons based on this iteration's filetype
    // listen for btn presses
    while let Some(event) = btn_rx.recv().await {
        match &mut state.current_state {
            DisplayState::Navigating => {
                match event {
                    ButtonEvent::Escape => {
                        // go up dir or show error msg
                        println!("Clicked escape!");
                    }
                    ButtonEvent::Select => {
                        // go into dir or show confirmmediaselection
                        println!("Clicked Select!");
                    }
                    ButtonEvent::Up => {
                        // goto prev file
                        println!("Clicked Up!");
                        if state.nav_state.current_index != 0 {
                            state.nav_state.current_index -= 1;
                        }
                    }
                    ButtonEvent::Down => {
                        // goto next file
                        println!("Clicked Down!");
                        // handle undraws, then draws based on new state

                        if state.nav_state.current_index != (state.nav_state.file_count - 1) {
                            state.nav_state.current_index += 1;
                        }
                    }
                }
            }
            DisplayState::ConfirmingMediaSelection => {
                match event {
                    ButtonEvent::Escape => {
                        // go back to navigation
                    }
                    ButtonEvent::Select => {
                        // go back or goto playing based on state
                        if state.modal_state.1 == true {
                            println!("hello world");
                            let draw_tx = draw_tx.clone();
                            tokio::spawn(async move {
                                let frame_bytes = WIDTH as usize * HEIGHT as usize * 2;
                                // 24 fps
                                let frame_delay = Duration::from_millis(42);
                                // open bgr565le file
                                // change to whatever current file ur on in dir
                                let mut dball = File::open("/home/yassin/cross_compiled/dragonball/goku_vs_piccolo_jr_le.raw").unwrap();
                                let mut frame = vec![0u8; frame_bytes];
                                while let Ok(()) = dball.read_exact(&mut frame) {
                                    // let started = Instant::now();
                                    draw_tx.send(DrawCommand::RawFrame { data: frame.clone() }).await.unwrap();
                                    thread::sleep(frame_delay);
                                    // let elapsed = started.elapsed();
                                    // if elapsed < frame_delay {
                                    //     thread::sleep(frame_delay - elapsed);
                                    // }
                                }
                            });
                        }
                    }
                    ButtonEvent::Up => {
                        // invert state
                    }
                    ButtonEvent::Down => {
                        // invert state
                    }
                }
            }
            DisplayState::PlayingSomething => {
                match event {
                    ButtonEvent::Escape => {
                        // set confirmingmediaexit state
                    }
                    ButtonEvent::Select => {
                        // pause media
                    }
                    ButtonEvent::Up => {
                        // turn up volume
                    }
                    ButtonEvent::Down => {
                        // turn down volume
                    }
                }
            }
            DisplayState::ConfirmingMediaExit => {
                match event {
                    ButtonEvent::Escape => {
                        // set current state to prev state
                    }
                    ButtonEvent::Select => {
                        // set current state to either navigating or playingmedia
                    }
                    ButtonEvent::Up => {
                        // invert state
                    }
                    ButtonEvent::Down => {
                        // invert state
                    }
                    _ => ()
                }
            }
            DisplayState::ErrorMessage => {
                match event {
                    ButtonEvent::Select => {
                        // back to previous_state
                    }
                    _ => ()
                }
            }
            DisplayState::UnrecoverableError => {
                match event {
                    ButtonEvent::Select => {
                        // shut down device
                    }
                    _ => ()
                }
            }
        }
    }
    loop {}
}

async fn button_task(chip_path: &str, gpio_number: u32, mut tx: mpsc::Sender<ButtonEvent>, event_type: ButtonEvent) {
    let mut chip = Chip::new(chip_path).unwrap();
    let mut db = debounce_4(false);

    loop {
        let pressed = tokio::task::spawn_blocking({
            let handle = chip
                .get_line(gpio_number)
                .unwrap()
                .request(LineRequestFlags::INPUT, 0, "btn")
                .unwrap();
            let mut pin = CdevPin::new(handle).unwrap();
            move || pin.is_low().unwrap()
        }).await.unwrap();

        if let Some(edge) = db.update(pressed) {
            if edge == Edge::Rising {
                tx.send(event_type).await.unwrap();
            }
        }
        sleep(Duration::from_millis(10)).await;
    }
}


// EVERYTHING HERE IS RELATED TO DRAWING ONLY, NO LOGIC
//
// drawtarget impl for framebufferdisplay
struct FramebufferDisplay<'a> {
    buf: &'a mut [u8],
    width: usize,
    height: usize,
}

impl<'a> OriginDimensions for FramebufferDisplay<'a> {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}
impl <'a> DrawTarget for FramebufferDisplay<'a> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
        I: IntoIterator<Item = Pixel<Self::Color>> 
    {
        for Pixel(coord, color) in pixels {
            if coord.x >= 0 && coord.x < self.width as i32 && coord.y >= 0 && coord.y < self.height as i32 {
                // 2 bytes per pixel (rgb565)
                let idx = (coord.y as usize * self.width + coord.x as usize) * 2;
                let mut value = color.into_storage();
                // rgb => bgr
                let red = (value >> 11) & 0x1F;
                let green = (value >> 5) & 0x3F;
                let blue = value & 0x1F;
                let swapped = (blue << 11) | (green << 5) | red;

                let rgb = swapped.swap_bytes().to_be_bytes();
                self.buf[idx] = rgb[0];
                self.buf[idx + 1] = rgb[1];
            }
        }
        Ok(())
    }
}

enum DrawCommand {
    // happens after selecting or exiting
    ConfirmingBackground {
        message: String,
        options: Vec<String>
    },
    // draw rect on top, 3 rects in middle/bottom for file icon, and file name after state change
    // to navigating
    NavigatingBackground {
        drawings: Vec<RoundedRectangle>,
    },
    // text only happens when navigatingbackground has been sent and state has changed to
    // navigating
    Text {
        content: String,
        position: Point
    },
    RawFrame {
        data: Vec<u8>,
    },
    ClearScreen
}
enum ControlCommand {
    Stop,
    Pause,
    Resume,
}
// light background, dark text
fn draw_modal(fb: &mut [u8], width: usize, height: usize, msg: &str, options: Vec<String>) {
    clear_screen(fb);
    // wipe screen first TODO()
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let style = PrimitiveStyleBuilder::new()
        .stroke_width(3)
        .stroke_color(Rgb565::BLACK)
        .fill_color(Rgb565::CSS_NAVAJO_WHITE)
        .build();

    Rectangle::new(Point::new(10, height as i32 / 2), Size::new(40, 20))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
fn undraw_modal(fb: &mut [u8], width: usize, height: usize, msg: &str) {
    let mut display = FramebufferDisplay { buf: fb, width, height };

    let style = PrimitiveStyleBuilder::new()
        .stroke_width(3)
        .stroke_color(Rgb565::BLACK)
        .fill_color(Rgb565::BLACK)
        .build();

    Rectangle::new(Point::new(10, height as i32 / 2), Size::new(40, 20))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}
// top rect, and 3 middle rects spaced out by 10px
fn draw_nav_background(fb: &mut [u8], width: usize, height: usize, drawings: Vec<RoundedRectangle>) {
    // wipe screen first TODO()
    clear_screen(fb);

    // draw when transitioning to navigating state
    let mut display = FramebufferDisplay { buf: fb, width, height };

    // top nav rectangle
    let style = PrimitiveStyleBuilder::new()
        .fill_color(Rgb565::CSS_NAVAJO_WHITE)
        .build();

    for drawing in drawings {
        drawing
            .into_styled(style)
            .draw(&mut display)
            .unwrap();
    }
    // add nav images on top. (folder, temperature, time icons)
    draw::draw_folder(fb, width, height, Point::new(14, 10));
    draw::draw_cloud(fb, width, height, Point::new(150, 10));
    draw::draw_clock(fb, width, height, Point::new(220, 10));

    // THESE ALL SHOULD PROBABLY BE HAND DRAWN, GPT5 CANT DRAW ICONS FOR SHIT LOL
    // draw::draw_camera(fb, width, height, Point::new(40, 100));
    // draw::draw_file(fb, width, height, Point::new(65, 100));
    // draw::draw_paused(fb, width, height, Point::new(80, 100));
    // draw::draw_volume(fb, width, height, Point::new(105, 100));
    // draw::draw_playing(fb, width, height, Point::new(130, 100));
    // draw::draw_question_mark(fb, width, height, Point::new(155, 100));

}
fn undraw_nav_background(fb: &mut [u8], width: usize, height: usize, msg: &str, point: Point) {
    // undraw when leaving navigating state
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let style = PrimitiveStyleBuilder::new()
        .stroke_width(3)
        .stroke_color(Rgb565::WHITE)
        .fill_color(Rgb565::WHITE)
        .build();

    Rectangle::new(point, Size::new(width as u32, height as u32))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}

fn draw_text(fb: &mut [u8], width: usize, height: usize, msg: &str, point: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);

    Text::with_baseline(msg, point, style, Baseline::Top)
        .draw(&mut display)
        .unwrap();
}
fn undraw_text(fb: &mut [u8], width: usize, height: usize, msg: &str, point: Point) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let style = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_NAVAJO_WHITE);

    Text::with_baseline(msg, point, style, Baseline::Top)
        .draw(&mut display)
        .unwrap();
}
fn clear_screen(fb: &mut [u8]) {
    let mut display = FramebufferDisplay { buf: fb, width: 320, height: 240 };

    let style = PrimitiveStyleBuilder::new()
        .fill_color(Rgb565::CSS_DARK_GRAY)
        .build();
    Rectangle::new(Point::zero(), Size::new(320, 240))
        .into_styled(style)
        .draw(&mut display)
        .unwrap();
}

fn draw_raw_frame(fb: &mut [u8], frame_data: &[u8]) {
    fb.copy_from_slice(frame_data);
}
fn format_dir(current_dir: PathBuf) -> PathBuf {
    if current_dir.to_str().unwrap() == "/home/yassin" {
        return PathBuf::from("/home");
    }
    else {
        return current_dir;
    }
}
async fn start_drawing_task(mut draw_rx: mpsc::Receiver<DrawCommand>) {
    tokio::task::spawn_blocking(move || {
        let fb = Framebuffer::new("/dev/fb1").expect("Failed to open framebuffer");
        let width = fb.get_size().0 as usize;
        let height = fb.get_size().1 as usize;
        let bpp = fb.get_bytes_per_pixel() as usize;
        let frame_size = width * height * bpp;

        let mut mapped = fb.map().expect("Failed to map framebuffer memory");
        let mut playing_video = false;
        let mut timestamp = 0;

        while let Some(cmd) = draw_rx.blocking_recv() {
            match cmd {
                DrawCommand::Text { content, position } => {
                    draw_text(&mut mapped, width, height, content.as_str(), position);
                },
                DrawCommand::ConfirmingBackground { message, options } => {
                    draw_modal(&mut mapped, width, height, &message, options);
                },
        // current dir, 
                DrawCommand::NavigatingBackground { drawings } => {
                    draw_nav_background(&mut mapped, width, height, drawings);
                }
                DrawCommand::RawFrame { data } => {
                    draw_raw_frame(&mut mapped, &data);
                },
                DrawCommand::ClearScreen => {
                    clear_screen(&mut mapped);
                }
            }
        }
    });
}

fn scroll_up(state: &mut State, draw_tx: mpsc::Sender<DrawCommand>) {
    // can animate this in future
    // lengths 2 and 3 need to be handled a bit differently compared to 4+

    // if current index is 0 = do nothing
    if state.nav_state.current_index > 0 {
        let mut readdir: Vec<_> = std::fs::read_dir(state.nav_state.current_dir.to_owned()).unwrap().collect::<Result<_, _>>().unwrap();
        readdir.reverse();
        if readdir.len() == 2 {
            // index can only be 1 at this point, draw 0 at middle and 1 on bottom
        }
        else if readdir.len() == 3 {
            // index could be 1 or 2 at this point
            // if index == 1, undraw 0/1/2, draw 0 middle, 1 on bottom
            // if index == 2, undraw 1/2, draw 0 top, 1 middle, 2 bottom
        }
        else {
            // len could be any between 4-69420

            // i need index, index+1, and index+2
            // undraw top and middle,
            if state.nav_state.current_index == (state.nav_state.file_count - 1) && readdir.len() > 2 {
                for (index, dir) in readdir.iter().enumerate().skip((readdir.len() - 1) - state.nav_state.current_index) {
                }
            }
            // undraw all
            // index-1 always exists because not the end,
            else {
                // if 2 or more indexes in front of me, get index-1, index, index+1, index+2
                // else, get index-1, index, index+1
                for (index, dir) in readdir.iter().enumerate().skip((readdir.len() - 1) - state.nav_state.current_index) {
                    if (readdir.len() - 1) - index > 1 {

                    }
                    else {

                    }
                }
            }
        }



        // set new states

        // draw what is next based on states
    }
    else {
        // do nothing?
    }
}
fn scroll_down(state: &mut State, draw_tx: mpsc::Sender<DrawCommand>) {

}


