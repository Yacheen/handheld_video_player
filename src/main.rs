use embedded_graphics::{
    mono_font::{ascii::{FONT_6X10, FONT_8X13}, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use embedded_hal::digital::{InputPin, OutputPin};
use linux_embedded_hal::gpio_cdev::{Chip, EventRequestFlags, EventType, LineRequestFlags, AsyncLineEventHandle};
use linux_embedded_hal::i2cdev::core::I2CDevice;
use linux_embedded_hal::{ CdevPin };
use std::{collections::HashMap, io::prelude::*, sync::{Arc, Mutex}};
use std::fs::File;
use std::thread;
use linuxfb::Framebuffer;
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
use tokio::{sync::mpsc, time::{sleep, Duration, Instant}};
use futures::StreamExt;
use debouncr::{debounce_4, Debouncer, Edge, Repeat4};
// const WIDTH: usize = 320;
// const HEIGHT: usize = 240;
// const FRAME_SIZE: usize = WIDTH * HEIGHT * 2;
const SSD1306_SLAVE_ADDR: u16 = 0x3c;

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
enum ButtonEvent {
    Up,
    Down,
    Select,
    Escape,
}
enum DisplayState {
    Navigating(NavigatingData),
    PlayingSomething(PlayingSomethingData),
    ConfirmingMediaSelection(bool),
    ConfirmingMediaExit(bool),
    UnrecoverableError(String),
    ErrorMessage(String),
}
struct State {
    current_state: DisplayState,
    previous_state: DisplayState,
}
struct NavigatingData {
    current_dir: String,
    files: Vec<String>,
    current_file_hovered: String,
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
    println!("Hello, world!");
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

    Text::with_baseline("Hello,", Point::zero(), text_style, Baseline::Top)
        .draw(&mut i2c_screen1_display)
        .unwrap();
    i2c_screen1_display.flush().unwrap();
    

    Text::with_baseline("Yassin", Point::zero(), text_style, Baseline::Top)
        .draw(&mut i2c_screen2_display)
        .unwrap();
    i2c_screen2_display.flush().unwrap();


    // delay init
    // let mut delay = Delay {};
    //
    // // spi0 tft ili9341 display------------------------------------------------------
    // // dc and rst
    // let mut dc = CdevPin::new(chip.get_line(27).unwrap().request(LineRequestFlags::OUTPUT, 0, "dc").unwrap()).unwrap();
    // let mut rst = CdevPin::new(chip.get_line(22).unwrap().request(LineRequestFlags::OUTPUT, 0, "rst").unwrap()).unwrap();

    //spi pins
    // let sclk = CdevPin::new(chip.get_line(11).unwrap().request(LineRequestFlags::OUTPUT, 0, "sclk").unwrap()).unwrap();
    // let mosi = CdevPin::new(chip.get_line(10).unwrap().request(LineRequestFlags::OUTPUT, 0, "mosi").unwrap()).unwrap();
    // let miso = CdevPin::new(chip.get_line(9).unwrap().request(LineRequestFlags::OUTPUT, 0, "miso").unwrap()).unwrap();
    let mut backlight = CdevPin::new(chip.get_line(5).unwrap().request(LineRequestFlags::OUTPUT, 1, "bl").unwrap()).unwrap();
    backlight.set_high().unwrap();

    // open framebuffer device, get properties
    let fb = Arc::new(Mutex::new(Framebuffer::new("/dev/fb1").unwrap()));
    let (width, height) = fb.lock().unwrap().get_size();
    let bpp = fb.lock().unwrap().get_bytes_per_pixel() as usize;
    let (vx, vy) = fb.lock().unwrap().get_virtual_size();
    eprintln!("fb: {}×{}, virtual {}×{}", width, height, vx, vy);

    // buttons
    // issues could possibly arrive being a buffer of 16 only idk tho
    let (tx, mut rx) = mpsc::channel(16);

    let navigating_data = NavigatingData {
        files: Vec::new(),
        current_dir: String::new(),
        current_file_hovered: String::new(),
    };
    let prev_navigating_data = NavigatingData {
        files: Vec::new(),
        current_dir: String::new(),
        current_file_hovered: String::new(),
    };
    let mut state = State {
        current_state: DisplayState::Navigating(navigating_data),
        previous_state: DisplayState::Navigating(prev_navigating_data),
    };

    // select
    tokio::spawn(button_task(chip_path, 19, tx.clone(), ButtonEvent::Select));
    // escape
    tokio::spawn(button_task(chip_path, 26, tx.clone(), ButtonEvent::Escape));
    // up
    tokio::spawn(button_task(chip_path, 13, tx.clone(), ButtonEvent::Up));
    // down
    tokio::spawn(button_task(chip_path, 6, tx.clone(), ButtonEvent::Down));

    
    // Start program and listen for button presses
    while let Some(event) = rx.recv().await {
        // check what state ur currently in
        // also check what buttonevent happened
        // finally, based on the previous state, do an action
        match &mut state.current_state {
            DisplayState::Navigating(navigating_data) => {
                match event {
                    ButtonEvent::Escape => {
                        // go up dir or show error msg
                        println!("Clicked escape!");
                    }
                    ButtonEvent::Select => {
                        // go into dir or show confirmmediaselection
                        println!("Clicked Select!");

                        let fb2 = fb.clone();
                        tokio::spawn(async move {
                            // map framebuffer mem once
                            let mut fb_mem = fb2.lock().unwrap().map().unwrap();
                            // bytes per file & and fps
                            let frame_bytes = width as usize * height as usize * bpp;
                            // 24 fps
                            let frame_delay = Duration::from_millis(42);
                            // open rgb565 file
                            let mut dball = File::open("dragonball/goku_vs_piccolo_jr_le.raw").unwrap();
                            let mut frame = vec![0u8; frame_bytes];

                            while let Ok(()) = dball.read_exact(&mut frame) {
                                let started = Instant::now();
                                fb_mem[..frame_bytes].copy_from_slice(&frame);

                                let elapsed = started.elapsed();
                                if elapsed < frame_delay {
                                    thread::sleep(frame_delay - elapsed);
                                }
                            }
                        });
                    }
                    ButtonEvent::Up => {
                        // goto prev file
                        println!("Clicked Up!");
                    }
                    ButtonEvent::Down => {
                        // goto next file
                        println!("Clicked Down!");
                    }
                }
            }
            DisplayState::ConfirmingMediaSelection(hover_state) => {
                match event {
                    ButtonEvent::Escape => {
                        // go back to navigation
                    }
                    ButtonEvent::Select => {
                        // go back or goto playing based on state
                    }
                    ButtonEvent::Up => {
                        // invert state
                    }
                    ButtonEvent::Down => {
                        // invert state
                    }
                }
            }
            DisplayState::PlayingSomething(media_data) => {
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
            DisplayState::ConfirmingMediaExit(hover_state) => {
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
            DisplayState::ErrorMessage(msg) => {
                match event {
                    ButtonEvent::Select => {
                        // back to previous_state
                    }
                    _ => ()
                }
            }
            DisplayState::UnrecoverableError(msg) => {
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
