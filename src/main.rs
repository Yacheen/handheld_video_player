use chrono::{DateTime, Local};
use embedded_graphics::{
    image::{Image, ImageRaw, ImageRawLE}, mono_font::{ascii::{FONT_6X10, FONT_8X13}, MonoTextStyle, MonoTextStyleBuilder}, pixelcolor::{BinaryColor, Rgb565}, prelude::*, primitives::{PrimitiveStyleBuilder, Rectangle, RoundedRectangle}, text::{Baseline, Text}
};
use embedded_hal::digital::{InputPin, OutputPin};
use linux_embedded_hal::{gpio_cdev::{AsyncLineEventHandle, Chip, EventRequestFlags, EventType, LineRequestFlags}, I2cdev};
use linux_embedded_hal::i2cdev::core::I2CDevice;
use linux_embedded_hal::{ CdevPin };
use std::{collections::HashMap, fs::DirEntry, io::prelude::*, os::unix::{ffi::OsStringExt, fs::MetadataExt}, path::PathBuf, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex}};
use std::fs::File;
use std::thread;
use linuxfb::Framebuffer;
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
use tokio::{sync::mpsc, time::{sleep, Duration, Instant}};
use futures::StreamExt;
use debouncr::{debounce_4, Debouncer, Edge, Repeat4};
use std::process::Command;

use crate::draw::{BOTTOM_CAROUSEL_ICON_COORDS, ENTRY_META_FILESIZE_TEXT_COORDS, MIDDLE_CAROUSEL_ICON_COORDS, TOP_CAROUSEL_ICON_COORDS};


const WIDTH: usize = 320;
const HEIGHT: usize = 240;
// const FRAME_SIZE: usize = WIDTH * HEIGHT * 2;
const SSD1306_SLAVE_ADDR: u16 = 0x3c;
const SCREEN_FPS: u8 = 24;
const PIXELS_PER_FRAME: u32 = 153_600;

// mods
mod draw;
mod utils;

#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
enum ButtonEvent {
    Up,
    Down,
    Select,
    Escape,
    TimeChanged,
    CurrentFrameChanged,
}
#[derive(Clone, Copy, Debug)]
enum DisplayState {
    Navigating,
    PlayingSomething,
    ConfirmingMediaSelection,
    ConfirmingMediaExit,
    UnrecoverableError,
    ErrorMessage,
}
struct State {
    current_state: Arc<tokio::sync::Mutex<DisplayState>>,
    previous_state: DisplayState,
    nav_state: NavigatingData,
    video_state: PlayingSomethingData,
    modal_state: Option<ModalState>,
    error_state: String,
    current_time: Arc<Mutex<DateTime<Local>>>,
}
#[derive(Clone)]
struct ModalState {
    message: String,
    selected: u8,
    file: Option<FileDetails>,
}
#[derive(Clone)]
struct FileDetails {
    file_path: PathBuf,
    file_size: u64,
    file_name: String,
    file_extension: String,
    is_dir: bool,
    last_modified: String,
}
struct NavigatingData {
    current_dir: PathBuf,
    current_index: usize,
    file_count: usize,
}
struct PlayingSomethingData {
    paused: Arc<AtomicBool>,
    // framerate is 24fps, so for example, frame 480 would be 20 seconds into the video
    current_frame: Arc<AtomicU64>,
    volume: Arc<AtomicU64>,
    total_frames: Arc<AtomicU64>,
    drawn_timestamp: String,
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

    // buttons channels and tasks-------------------------------------------------------------
    // btn channel
    let (btn_tx, mut btn_rx) = mpsc::channel(128);
    // draw channel
    let (draw_tx, mut draw_rx) = mpsc::channel::<DrawCommand>(128);
    let (i2c_draw_tx, mut i2c_draw_rx) = mpsc::channel::<DrawCommand>(128);
    // video/music task command channel (pause, resume, stop), prob wont use anymore
    // let (media_tx, mut media_rx) = mpsc::channel::<ControlCommand>(128);

    let file_count = std::fs::read_dir(std::env::current_dir().unwrap().as_path()).unwrap().count();
    // this'll give you: 2069-01-24 13:17:44.609871 UTC or something.
    let current_local_time: DateTime<Local> = Local::now();
    let formatted_local_time = current_local_time.format("%-I:%M%P").to_string();
    println!("file count!: {}", file_count);
    println!("formatted local time: {:?}", formatted_local_time);
    draw_tx.send(DrawCommand::Text { content: formatted_local_time, position: draw::TOP_NAV_CLOCK_TEXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();


    let mut state = State {
        current_state: Arc::new(tokio::sync::Mutex::new(DisplayState::Navigating)),
        previous_state: DisplayState::Navigating,
        nav_state: NavigatingData {
            current_dir: current_dir.clone(),
            file_count: file_count,
            current_index: 0,
        },
        video_state: PlayingSomethingData {
            paused: Arc::new(AtomicBool::new(false)),
            current_frame: Arc::new(AtomicU64::new(0)),
            total_frames: Arc::new(AtomicU64::new(0)),
            volume: Arc::new(AtomicU64::new(0)),
            drawn_timestamp: String::from("0:00 / 0:00"),
        },
        modal_state: None,
        error_state: String::new(),
        current_time: Arc::new(Mutex::new(current_local_time)),
    };

    // select
    tokio::spawn(button_task(chip_path, 19, btn_tx.clone(), ButtonEvent::Select));
    // escape
    tokio::spawn(button_task(chip_path, 26, btn_tx.clone(), ButtonEvent::Escape));
    // up
    tokio::spawn(button_task(chip_path, 13, btn_tx.clone(), ButtonEvent::Up));
    // down
    tokio::spawn(button_task(chip_path, 6, btn_tx.clone(), ButtonEvent::Down));
    // time changer
    tokio::spawn(current_time_task(btn_tx.clone(), state.current_time.clone(), state.current_state.clone()));
    // watch frames and change timestamp on 2nd screen when applicable
    tokio::spawn(current_frame_task(btn_tx.clone(), state.video_state.current_frame.clone(), state.video_state.total_frames.clone(), state.video_state.paused.clone()));

    // draw task - will draw whatever until end of program
    tokio::spawn(start_drawing_task(draw_rx));

    // wait for tasks to be ready or something idk, maybe mostly drawing task to init i2c and spi
    std::thread::sleep(Duration::from_millis(200)); 

    draw_tx.send(DrawCommand::ClearScreen).await.unwrap();
    std::thread::sleep(Duration::from_millis(200));

    draw_tx.send(DrawCommand::NavigatingBackground { current_dir: current_dir.clone(), file_count: file_count, current_index: state.nav_state.current_index }).await.unwrap();

    // listen for btn presses
    while let Some(event) = btn_rx.recv().await {
        let current_state = *state.current_state.lock().await;
        match current_state {
            DisplayState::Navigating => {
                match event {
                    ButtonEvent::Escape => {
                        // go up dir or show error msg
                        println!("Clicked escape!");
                        // TODO!() use Result type instead
                        let res = exit_dir(&state.nav_state, draw_tx.clone()).await;
                        match res {
                            Some((path, file_count)) => {
                                state.nav_state.current_dir = path;
                                state.nav_state.current_index = 0;
                                state.nav_state.file_count = file_count;
                            }
                            None => {
                                // show error? idk, maybe use Err(msg) instead
                                {
                                    let current_state = state.current_state.clone();
                                    let mut current_state = current_state.lock().await;
                                    *current_state = DisplayState::ErrorMessage;
                                }
                                // set modal state to none here if u want idk
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Error! x_x".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::ConfirmingBackground { message: format!("Unknown error..."), options: vec!["Okay".to_string()] }).await.unwrap();
                            }
                        }
                    }
                    ButtonEvent::Select => {
                        // go into dir or show confirmmediaselection
                        println!("Clicked Select!");
                        let res = enter_dir_or_select_file(&state.nav_state, draw_tx.clone()).await;
                        match res {
                            SelectResponse::File { file_name, file_size, file_extension, file_path, last_modified } => {
                                println!("this file extension is: {}", file_extension);
                                println!("file size: {}", file_size);
                                println!("file name: {}", file_name);
                                let draw_tx = draw_tx.clone();
                                match file_extension.as_str() {
                                    "rgb565" | "raw" => {
                                        state.modal_state = Some(ModalState { message: format!("Play video: {}?", file_name), selected: 0, file: Some(FileDetails { file_path, file_size, file_name: file_name.clone(), file_extension: file_extension.clone(), is_dir: false, last_modified })});
                                        state.video_state.total_frames.swap(file_size, Ordering::Relaxed);
                                        {
                                            let current_state = state.current_state.clone();
                                            let mut current_state = current_state.lock().await;
                                            *current_state = DisplayState::ConfirmingMediaSelection;
                                        }
                                        draw_tx.send(DrawCommand::ConfirmingBackground { message: format!("Play video: {}?", file_name), options: vec!["No!".to_string(), "Yes!".to_string()] }).await.unwrap();
                                        draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                        draw_tx.send(DrawCommand::DrawI2CText { content: "Confirm?".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                                    }
                                    _ =>  {
                                        state.modal_state = Some(ModalState { message: "Can not currently play this kind of file - handling of different files (such as txt's, and other basic formats) are in development!".to_string(), selected: 0, file:Some(FileDetails { file_path, file_size, file_name: file_name.clone(), file_extension: file_extension.clone(), last_modified, is_dir: false })});
                                        {
                                            let current_state = state.current_state.clone();
                                            let mut current_state = current_state.lock().await;
                                            *current_state = DisplayState::ErrorMessage;
                                        }
                                        draw_tx.send(DrawCommand::ConfirmingBackground { message: "Can not currently play this kind of file - handling of different files (such as txt's, and other basic formats) are in development!".to_string(), options: vec!["Okay".to_string()] }).await.unwrap();
                                        draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                        draw_tx.send(DrawCommand::DrawI2CText { content: "Error! x_x".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                                    }
                                }
                            }
                            SelectResponse::Directory { file_path, file_count, last_modified } => {
                                state.nav_state.current_dir = file_path;
                                state.nav_state.current_index = 0;
                                state.nav_state.file_count = file_count;
                            }
                            SelectResponse::Error(err_msg) => {
                                state.modal_state = Some(ModalState { message: err_msg.clone(), selected: 0, file: None });
                                {
                                    let current_state = state.current_state.clone();
                                    let mut current_state = current_state.lock().await;
                                    *current_state = DisplayState::ErrorMessage;
                                }
                                draw_tx.send(DrawCommand::ConfirmingBackground { message: err_msg, options: vec!["Okay".to_string()] }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Error! x_x".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                            }
                            SelectResponse::FatalError(fatal_err_msg) => {
                                {
                                    let current_state = state.current_state.clone();
                                    let mut current_state = current_state.lock().await;
                                    *current_state = DisplayState::UnrecoverableError;
                                }
                                state.modal_state = Some(ModalState { message: fatal_err_msg.clone(), selected: 0, file: None });
                                draw_tx.send(DrawCommand::ConfirmingBackground { message: fatal_err_msg, options: vec!["Okay".to_string()] }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "FATAL ERROR!!".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                            }
                        }
                    }
                    ButtonEvent::Up => {
                        // goto prev file
                        println!("Clicked Up!");
                        if state.nav_state.file_count > 1 {
                            if state.nav_state.current_index != 0 {
                                let draw_tx = draw_tx.clone();
                                scroll_up(&state.nav_state, draw_tx).await;
                                state.nav_state.current_index -= 1;
                            }
                        }
                    }
                    ButtonEvent::Down => {
                        // goto next file
                        println!("Clicked Down!");
                        if state.nav_state.file_count > 1 {
                            if state.nav_state.current_index != (state.nav_state.file_count - 1) {
                                let draw_tx = draw_tx.clone();
                                scroll_down(&state.nav_state, draw_tx).await;
                                state.nav_state.current_index += 1;
                            }
                        }
                    }
                    ButtonEvent::TimeChanged => {
                        {
                            let current_time = state.current_time.lock().unwrap();
                            draw_tx.send(DrawCommand::Text { content: current_time.format("%-I:%M%P").to_string(), position: draw::TOP_NAV_CLOCK_TEXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
                        }
                        let new_current_local_time: DateTime<Local> = Local::now();
                        let new_formatted_local_time = new_current_local_time.format("%-I:%M%P").to_string();
                        {
                            let mut current_time = state.current_time.lock().unwrap();
                            *current_time = new_current_local_time;
                        }
                        draw_tx.send(DrawCommand::Text { content: new_formatted_local_time, position: draw::TOP_NAV_CLOCK_TEXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();
                    }
                    ButtonEvent::CurrentFrameChanged => {}
                }
            }
            DisplayState::ConfirmingMediaSelection => {
                match event {
                    ButtonEvent::Escape => {
                        // go back to navigation
                        {
                            let current_state = state.current_state.clone();
                            let mut current_state = current_state.lock().await;
                            *current_state = DisplayState::Navigating;
                        }
                        // set modal state to none here if u want idk
                        if let Some(modal_state) = &mut state.modal_state {
                            modal_state.selected = 0;
                        }
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Confirm?".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                        draw_tx.send(DrawCommand::NavigatingBackground { current_dir: state.nav_state.current_dir.clone(), file_count: state.nav_state.file_count, current_index: state.nav_state.current_index }).await.unwrap();
                    }
                    ButtonEvent::Select => {
                        // go back or goto playing based on state
                        if let Some(modal_state) = &mut state.modal_state {
                            if modal_state.selected == 0 {
                                {
                                    let current_state = state.current_state.clone();
                                    let mut current_state = current_state.lock().await;
                                    *current_state = DisplayState::Navigating;
                                }
                                // set modal state to none here if u want idk
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Confirm?".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::NavigatingBackground { current_dir: state.nav_state.current_dir.clone(), file_count: state.nav_state.file_count, current_index: state.nav_state.current_index }).await.unwrap();
                            }
                            else if modal_state.selected == 1 {
                                let draw_tx = draw_tx.clone();
                                let current_state = state.current_state.clone(); 
                                state.video_state.paused.store(false, Ordering::Relaxed);
                                {
                                    let mut current_state = current_state.lock().await;
                                    *current_state = DisplayState::PlayingSomething;
                                }
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Confirm?".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Playing media!".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                                let draw_tx = draw_tx.clone();
                                let file_details = modal_state.file.clone();
                                let paused = state.video_state.paused.clone();
                                let current_frame = state.video_state.current_frame.clone();
                                tokio::spawn(play_video(current_frame.clone(), paused.clone(), file_details, draw_tx.clone()));
                            }
                            modal_state.selected = 0;
                        }
                    }
                    ButtonEvent::Up => {
                        if let Some(modal_state) = &mut state.modal_state {
                            modal_state.selected = 0;
                            draw_tx.send(DrawCommand::SelectNo).await.unwrap();
                        }
                    }
                    ButtonEvent::Down => {
                        if let Some(modal_state) = &mut state.modal_state {
                            modal_state.selected = 1;
                            draw_tx.send(DrawCommand::SelectYes).await.unwrap();
                        }
                    }
                    ButtonEvent::TimeChanged => {
                        let new_current_local_time: DateTime<Local> = Local::now();
                        {
                            let mut current_time = state.current_time.lock().unwrap();
                            *current_time = new_current_local_time;
                        }
                    }
                    ButtonEvent::CurrentFrameChanged => {}
                }
            }
            DisplayState::PlayingSomething => {
                match event {
                    ButtonEvent::Escape => {
                        // set confirmingmediaexit state and set paused
                        state.video_state.paused.store(true, Ordering::Relaxed);
                        {
                            let current_state = state.current_state.clone();
                            let mut current_state = current_state.lock().await;
                            *current_state = DisplayState::ConfirmingMediaExit;
                        }
                        // there should always be a modal state at this point (file is set during
                        // confirmmediaselection)
                        if let Some(modal_state) = &mut state.modal_state {
                            modal_state.selected = 0;
                        }
                        draw_tx.send(DrawCommand::ConfirmingBackground { message: format!("Exit to navigation menu?"), options: vec!["No!".to_string(), "Yes!".to_string()] }).await.unwrap();
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Playing media!".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Exit media?".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                    }
                    ButtonEvent::Select => {
                        // pause media
                    }
                    ButtonEvent::Up => {
                        // turn up volume
                        let volume = state.video_state.volume.load(Ordering::Relaxed);
                        if volume != 100 {
                            draw_tx.send(DrawCommand::DrawI2CText { content: format!("{}%",state.video_state.volume.load(Ordering::Relaxed).to_string()), position: draw::TOP_VOLUME_VALUE_COORDS, undraw: true, screen: true }).await.unwrap();
                            state.video_state.volume.fetch_add(5, Ordering::Relaxed);
                            draw_tx.send(DrawCommand::DrawI2CText { content: format!("{}%",state.video_state.volume.load(Ordering::Relaxed).to_string()), position: draw::TOP_VOLUME_VALUE_COORDS, undraw: false, screen: true }).await.unwrap();
                        }
                    }
                    ButtonEvent::Down => {
                        // turn down volume
                        let volume = state.video_state.volume.load(Ordering::Relaxed);
                        if volume != 0 {
                            draw_tx.send(DrawCommand::DrawI2CText { content: format!("{}%",state.video_state.volume.load(Ordering::Relaxed).to_string()), position: draw::TOP_VOLUME_VALUE_COORDS, undraw: true, screen: true }).await.unwrap();
                            state.video_state.volume.fetch_sub(5, Ordering::Relaxed);
                            draw_tx.send(DrawCommand::DrawI2CText { content: format!("{}%",state.video_state.volume.load(Ordering::Relaxed).to_string()), position: draw::TOP_VOLUME_VALUE_COORDS, undraw: false, screen: true }).await.unwrap();
                        }
                    }
                    ButtonEvent::TimeChanged => {
                        let new_current_local_time: DateTime<Local> = Local::now();
                        {
                            let mut current_time = state.current_time.lock().unwrap();
                            *current_time = new_current_local_time;
                        }
                    }
                    ButtonEvent::CurrentFrameChanged => {
                        // draw timestamp to i2c display 2 at point 

                        // let current_frame = state.video_state.current_frame.load(Ordering::Relaxed);
                        // undraw
                        draw_tx.send(DrawCommand::DrawI2CText { content: state.video_state.drawn_timestamp, position: draw::TOP_MEDIA_TIMESTAMP_COORDS, undraw: true, screen: true }).await.unwrap();
                        
                        // change states
                        let new_timestamp = utils::format_timecode(state.video_state.current_frame.load(Ordering::Relaxed), state.video_state.total_frames.load(Ordering::Relaxed) / PIXELS_PER_FRAME as u64, SCREEN_FPS as u64);
                        state.video_state.drawn_timestamp = new_timestamp.clone();

                        // draw
                        draw_tx.send(DrawCommand::DrawI2CText { content: new_timestamp, position: draw::TOP_MEDIA_TIMESTAMP_COORDS, undraw: false, screen: true }).await.unwrap();
                    }
                }
            }
            DisplayState::ConfirmingMediaExit => {
                match event {
                    ButtonEvent::Escape => {
                        println!("pressing esc in exit!");
                        // resume video TODO!()
                        if let Some(modal_state) = &mut state.modal_state {
                            // resume video TODO!()
                            state.video_state.paused.swap(false, Ordering::Relaxed);
                            {
                                let current_state = state.current_state.clone();
                                let mut current_state = current_state.lock().await;
                                *current_state = DisplayState::PlayingSomething;
                            }
                            modal_state.selected = 0;
                            draw_tx.send(DrawCommand::DrawI2CText { content: "Exit media?".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                            draw_tx.send(DrawCommand::DrawI2CText { content: "Playing media!".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();

                            tokio::spawn(play_video(state.video_state.current_frame.clone(), state.video_state.paused.clone(), modal_state.file.clone(), draw_tx.clone()));
                            // draw_tx.send(DrawCommand::NavigatingBackground { current_dir: state.nav_state.current_dir.clone(), file_count: state.nav_state.file_count, current_index: state.nav_state.current_index }).await.unwrap();
                        }
                    }
                    ButtonEvent::Select => {
                        println!("pressing select in exit!");
                        // set current state to either navigating or playingmedia
                        if let Some(modal_state) = &mut state.modal_state {
                            if modal_state.selected == 0 {
                                // resume video TODO!()
                                {
                                    let current_state = state.current_state.clone();
                                    let mut current_state = current_state.lock().await;
                                    *current_state = DisplayState::PlayingSomething;
                                }
                                state.video_state.paused.swap(false, Ordering::Relaxed);
                                modal_state.selected = 0;
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Exit media?".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Playing media!".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();

                                tokio::spawn(play_video(state.video_state.current_frame.clone(), state.video_state.paused.clone(), modal_state.file.clone(), draw_tx.clone()));
                                // draw_tx.send(DrawCommand::NavigatingBackground { current_dir: state.nav_state.current_dir.clone(), file_count: state.nav_state.file_count, current_index: state.nav_state.current_index }).await.unwrap();
                            }
                            else if modal_state.selected == 1 {
                                // go back to navigation
                                {
                                    let current_state = state.current_state.clone();
                                    let mut current_state = current_state.lock().await;
                                    *current_state = DisplayState::Navigating;
                                }
                                modal_state.selected = 0;
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Exit media?".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                                draw_tx.send(DrawCommand::NavigatingBackground { current_dir: state.nav_state.current_dir.clone(), file_count: state.nav_state.file_count, current_index: state.nav_state.current_index }).await.unwrap();
                            }
                        }
                    }
                    ButtonEvent::Up => {
                        println!("pressing up in exit!");
                        // invert state
                        if let Some(modal_state) = &mut state.modal_state {
                            modal_state.selected = 0;
                            draw_tx.send(DrawCommand::SelectNo).await.unwrap();
                        }
                    }
                    ButtonEvent::Down => {
                        println!("pressing down in exit!");
                        // invert state
                        if let Some(modal_state) = &mut state.modal_state {
                            modal_state.selected = 1;
                            draw_tx.send(DrawCommand::SelectYes).await.unwrap();
                        }
                    }
                    ButtonEvent::TimeChanged => {
                        let new_current_local_time: DateTime<Local> = Local::now();
                        {
                            let mut current_time = state.current_time.lock().unwrap();
                            *current_time = new_current_local_time;
                        }
                    }
                    ButtonEvent::CurrentFrameChanged => {
                        println!("frame changed in exit!");

                    }
                    _ => ()
                }
            }
            DisplayState::ErrorMessage => {
                match event {
                    ButtonEvent::Escape => {
                        // go back to navigating
                        {
                            let current_state = state.current_state.clone();
                            let mut current_state = current_state.lock().await;
                            *current_state = DisplayState::Navigating;
                        }
                        // set modal state to none here if u want idk
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Error! x_x".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                        draw_tx.send(DrawCommand::NavigatingBackground { current_dir: state.nav_state.current_dir.clone(), file_count: state.nav_state.file_count, current_index: state.nav_state.current_index }).await.unwrap();
                    }
                    ButtonEvent::Select => {
                        // go back to navigating
                        {
                            let current_state = state.current_state.clone();
                            let mut current_state = current_state.lock().await;
                            *current_state = DisplayState::Navigating;
                        }
                        // set modal state to none here if u want idk
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Error! x_x".to_string(), position: Point::zero(), undraw: true, screen: false }).await.unwrap();
                        draw_tx.send(DrawCommand::DrawI2CText { content: "Navigating".to_string(), position: Point::zero(), undraw: false, screen: false }).await.unwrap();
                        draw_tx.send(DrawCommand::NavigatingBackground { current_dir: state.nav_state.current_dir.clone(), file_count: state.nav_state.file_count, current_index: state.nav_state.current_index }).await.unwrap();
                    }
                    ButtonEvent::TimeChanged => {
                        let new_current_local_time: DateTime<Local> = Local::now();
                        {
                            let mut current_time = state.current_time.lock().unwrap();
                            *current_time = new_current_local_time;
                        }
                    }
                    _ => ()
                }
            }
            DisplayState::UnrecoverableError => {
                match event {
                    ButtonEvent::Select => {
                        // shut down device
                        println!("Shutting down service!");
                        std::process::exit(0);
                    }
                    ButtonEvent::TimeChanged => {
                        let new_current_local_time: DateTime<Local> = Local::now();
                        {
                            let mut current_time = state.current_time.lock().unwrap();
                            *current_time = new_current_local_time;
                        }
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
async fn current_time_task(tx: mpsc::Sender<ButtonEvent>, state: Arc<Mutex<DateTime<Local>>>, current_state: Arc<tokio::sync::Mutex<DisplayState>>) {
    loop {
        match *current_state.lock().await {
            DisplayState::Navigating => {
                let new_current_local_time: DateTime<Local> = Local::now();
                if new_current_local_time != *state.lock().unwrap() {
                    tx.send(ButtonEvent::TimeChanged).await.unwrap();
                }
            }
            _ => ()
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
async fn current_frame_task(tx: mpsc::Sender<ButtonEvent>, current_frame: Arc<AtomicU64>, file_size: Arc<AtomicU64>, paused: Arc<AtomicBool>) {
    loop {
        let paused = paused.load(Ordering::Relaxed);
        if !paused {
            let current_frame = current_frame.load(Ordering::Relaxed);
            let total_frames = file_size.load(Ordering::Relaxed) / PIXELS_PER_FRAME as u64;
            // send timestamp changed
            if current_frame > 0 && current_frame % 24 == 0 && current_frame != total_frames {
                tx.send(ButtonEvent::CurrentFrameChanged).await.unwrap();
                println!("frame changed");
            }
            tokio::time::sleep(Duration::from_millis(42)).await;
        }
    }
}


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
        current_dir: PathBuf,
        file_count: usize,
        current_index: usize,
    },
    // text only happens when navigatingbackground has been sent and state has changed to
    // navigating
    Text {
        content: String,
        position: Point,
        undraw: bool,
        is_selected: bool,
    },
    RawFrame {
        data: Vec<u8>,
    },
    ClearScreen,
    DrawI2CText {
        content: String,
        position: Point,
        undraw: bool,
        screen: bool,
    },
    ClearI2CScreen(bool),
    SelectYes,
    SelectNo,
    DrawIcon {
        point: Point,
        kind: IconKind,
        undraw: bool,
    }
}
enum IconKind {
    Txt,
    Video,
    Folder,
    Questionmark
}
// light background, dark text
fn draw_modal(fb: &mut [u8], width: usize, height: usize, msg: &str, options: Vec<String>) {
    
    clear_screen(fb);

    // add error msg, line break every 30 characters, map options on bottom spaced based on len
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let modal_style = PrimitiveStyleBuilder::new()
        .stroke_width(2)
        .stroke_color(Rgb565::WHITE)
        .build();

    // border of modal
    RoundedRectangle::with_equal_corners(
        Rectangle::new(Point::new(40, 40), Size::new(240, 160)),
        Size::new(10, 10),
    )
    .into_styled(modal_style)
    .draw(&mut display)
    .unwrap();

    let mut formatted_msg = String::new();
    for (index, character) in msg.chars().enumerate() {
        formatted_msg += character.to_string().as_str();
        if (index + 1) % 30 == 0 && (index + 1) != msg.len() {
            formatted_msg += "\n";
        }
    }

    // parse msg, break it every 25 chars
    let txt_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    Text::with_baseline(formatted_msg.as_str(), Point::new(60, 60), txt_style, Baseline::Top)
        .draw(&mut display)
        .unwrap();

    // options
    let option_style = PrimitiveStyleBuilder::new()
        .stroke_width(1)
        .stroke_color(Rgb565::CSS_SKY_BLUE)
        .build();

    // 6 padding on left, 4 on top
    let num_of_options = options.iter().count();
    if num_of_options == 1 {
        // "okay" option
        RoundedRectangle::with_equal_corners(
            Rectangle::new(Point::new(140, 160), Size::new(40, 20)),
            Size::new(6, 6),
        )
        .into_styled(option_style)
        .draw(&mut display)
        .unwrap();
        Text::with_baseline("Okay!", Point::new(146, 164), txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
    else if num_of_options == 2 {
        // no option
        Rectangle::new(draw::MODAL_NO_BORDER_COORDS, Size::new(40, 20))
            .into_styled(option_style)
            .draw(&mut display)
            .unwrap();
        Text::with_baseline("No!", Point::new(116, 164), txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();

        // yes option
        Text::with_baseline("Yes!", Point::new(176, 164), txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
    // else if num_of_options == 3 {
    //
    // }
    // else {
    //
    // }
}
fn select_yes(fb: &mut [u8], width: usize, height: usize) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let selected_style = PrimitiveStyleBuilder::new()
        .stroke_width(1)
        .stroke_color(Rgb565::CSS_SKY_BLUE)
        .build();
    let clear_style = PrimitiveStyleBuilder::new()
        .stroke_width(1)
        .stroke_color(Rgb565::CSS_DARK_GRAY)
        .build();

    Rectangle::new(draw::MODAL_YES_BORDER_COORDS, Size::new(40, 20))
        .into_styled(selected_style)
        .draw(&mut display)
        .unwrap();
    Rectangle::new(draw::MODAL_NO_BORDER_COORDS, Size::new(40, 20))
        .into_styled(clear_style)
        .draw(&mut display)
        .unwrap();
}
fn select_no(fb: &mut [u8], width: usize, height: usize) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    let selected_style = PrimitiveStyleBuilder::new()
        .stroke_width(1)
        .stroke_color(Rgb565::CSS_SKY_BLUE)
        .build();
    let clear_style = PrimitiveStyleBuilder::new()
        .stroke_width(1)
        .stroke_color(Rgb565::CSS_DARK_GRAY)
        .build();

    Rectangle::new(draw::MODAL_YES_BORDER_COORDS, Size::new(40, 20))
        .into_styled(clear_style)
        .draw(&mut display)
        .unwrap();
    Rectangle::new(draw::MODAL_NO_BORDER_COORDS, Size::new(40, 20))
        .into_styled(selected_style)
        .draw(&mut display)
        .unwrap();
}
// fn undraw_modal(fb: &mut [u8], width: usize, height: usize, msg: &str) {
//     let mut display = FramebufferDisplay { buf: fb, width, height };
//
//     let style = PrimitiveStyleBuilder::new()
//         .stroke_width(3)
//         .stroke_color(Rgb565::BLACK)
//         .fill_color(Rgb565::BLACK)
//         .build();
//
//     Rectangle::new(Point::new(10, height as i32 / 2), Size::new(40, 20))
//         .into_styled(style)
//         .draw(&mut display)
//         .unwrap();
// }
// top rect, and 3 middle rects spaced out by 10px
fn draw_nav_background(fb: &mut [u8], width: usize, height: usize, current_dir: PathBuf, file_count: usize, current_index: usize) {
    // wipe screen first TODO()
    clear_screen(fb);

    // draw when transitioning to navigating state
    let mut display = FramebufferDisplay { buf: fb, width, height };

    // top nav rectangle
    let nav_style = PrimitiveStyleBuilder::new()
        .stroke_width(1)
        .stroke_color(Rgb565::WHITE)
        .build();

    let selected_style = PrimitiveStyleBuilder::new()
        .stroke_width(2)
        .stroke_color(Rgb565::CSS_SKY_BLUE)
        .build();

    let carousel_style = PrimitiveStyleBuilder::new()
        .stroke_width(2)
        .stroke_color(Rgb565::CSS_LIGHT_BLUE)
        .build();

    // top meta
    RoundedRectangle::with_equal_corners(
        Rectangle::new(draw::TOP_NAV_BG_COORDS, Size::new(300, 40)),
        Size::new(10, 10),
    )
    .into_styled(nav_style)
    .draw(&mut display)
    .unwrap();

    // carousel 
    RoundedRectangle::with_equal_corners(
        Rectangle::new(draw::CAROUSEL_CONTAINER_BG_COORDS, Size::new(270, 140)),
        Size::new(12, 12),
    )
    .into_styled(carousel_style)
    .draw(&mut display)
    .unwrap();

    // selected item in carousel
    RoundedRectangle::with_equal_corners(
        Rectangle::new(draw::MIDDLE_CAROUSEL_BG_COORDS, Size::new(250, 40)),
        Size::new(12, 12),
    )
    .into_styled(selected_style)
    .draw(&mut display)
    .unwrap();

    // add nav images on top. (folder, temperature, time icons)
    draw::draw_folder(&mut display, width, height, Point::new(14, 10));
    draw::draw_cloud(&mut display, width, height, Point::new(150, 10));
    draw::draw_clock(&mut display, width, height, Point::new(220, 10));

    // current path, file_count, and current file navigated on (index of files of dir)
    let txt_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);

    // current_dir
    Text::with_baseline(format_dir(current_dir.to_owned()).as_str(), draw::TOP_NAV_PATH_COORDS, txt_style, Baseline::Top)
        .draw(&mut display)
        .unwrap();

    // file_count
    if file_count == 0 {
        Text::with_baseline(format!("0/{}", file_count).as_str(), draw::TOP_NAV_FILE_INDEX_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
    else {
        Text::with_baseline(format!("1/{}", file_count).as_str(), draw::TOP_NAV_FILE_INDEX_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }

    // get idx, idx - 1, and idx + 1, if exists, draw in appropriate spots.
    let readdir: Vec<_> = std::fs::read_dir(current_dir.to_owned()).unwrap().collect::<Result<_, _>>().unwrap();
    let idx_minus_one = { if current_index == 0 { None } else { readdir.get(current_index - 1) } };
    let current_idx = readdir.get(current_index);
    let idx_plus_one = readdir.get(current_index + 1);


    if let Some(idx_minus_one) = idx_minus_one {
        let kind = determine_icon_to_draw(idx_minus_one);
        draw_icon(TOP_CAROUSEL_ICON_COORDS, &mut display, kind);
        Text::with_baseline(idx_minus_one.file_name().to_str().unwrap(), draw::TOP_CAROUSEL_TXT_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
    if let Some(current_idx) = current_idx {
        let kind = determine_icon_to_draw(current_idx);
        draw_icon(MIDDLE_CAROUSEL_ICON_COORDS, &mut display, kind);
        // file size, 
        Text::with_baseline("Size", draw::ENTRY_META_FILESIZE_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
        Text::with_baseline(&utils::format_bytes(current_idx.metadata().unwrap().size()), draw::ENTRY_META_FILESIZE_TEXT_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();

        // last_modified
        let mtime = current_idx.metadata().unwrap().mtime();
        let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
        let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
        Text::with_baseline("Last modified", draw::ENTRY_META_LASTMODIFIED_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
        Text::with_baseline(&last_modified, draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
        // selected file name
        Text::with_baseline(current_idx.file_name().to_str().unwrap(), draw::MIDDLE_CAROUSEL_TXT_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
    if let Some(idx_plus_one) = idx_plus_one {
        let kind = determine_icon_to_draw(idx_plus_one);
        draw_icon(BOTTOM_CAROUSEL_ICON_COORDS, &mut display, kind);
        Text::with_baseline(idx_plus_one.file_name().to_str().unwrap(), draw::BOTTOM_CAROUSEL_TXT_COORDS, txt_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
}
fn determine_icon_to_draw(direntry: &DirEntry) -> IconKind {
    if let Ok(file_type) = direntry.file_type() {
        if file_type.is_dir() {
            // draw folder_icon
            IconKind::Folder
        }
        else {
            // not a dir, can only be a file with/without an extension. .bashrc doesnt have an ext.
            if let Some(extension) = direntry.path().extension() {
                if let Some(ext) = extension.to_str() {
                    println!("extension: {}", ext);
                }
                match  extension.to_str().unwrap() {
                    "txt" | "bashrc" | "rs" | "sh" => {
                        // draw txt icon
                        IconKind::Txt
                    },
                    "rgb" | "raw" | "rgb565" | "mp4" => {
                        // draw video icon
                        IconKind::Video
                    },
                    _ => {
                        // draw questionmark icon
                        IconKind::Questionmark
                    }
                }
            }
            else {
                // draw questionmark icon
                IconKind::Questionmark
            }
        }
    }
    else {
        // draw questionmark icon
        IconKind::Questionmark
    }

}
fn draw_icon(point: Point, display: &mut FramebufferDisplay, kind: IconKind) {
    // icons
    // each icon is 20widthx24height
    // 20height x 24width x 2
    // 960bytes total
    // .. I think
    let folder_icon_file = std::fs::read("/home/yassin/cross_compiled/filetype_icons/folder_icon.rgb").unwrap();
    let video_icon_file = std::fs::read("/home/yassin/cross_compiled/filetype_icons/video_icon.rgb").unwrap();
    let questionmark_icon_file = std::fs::read("/home/yassin/cross_compiled/filetype_icons/questionmark_icon.rgb").unwrap();
    let txtfile_icon_file = std::fs::read("/home/yassin/cross_compiled/filetype_icons/txtfile_icon.rgb").unwrap();
    match kind {
        IconKind::Folder => {
            let raw: ImageRawLE<Rgb565> = ImageRaw::new(&folder_icon_file, 24);
            let image = Image::new(&raw, point);
            image.draw(display).unwrap();
        }
        IconKind::Video => {
            let raw: ImageRawLE<Rgb565> = ImageRaw::new(&video_icon_file, 24);
            let image = Image::new(&raw, point);
            image.draw(display).unwrap();
        }
        IconKind::Txt => {
            let raw: ImageRawLE<Rgb565> = ImageRaw::new(&txtfile_icon_file, 24);
            let image = Image::new(&raw, point);
            image.draw(display).unwrap();
        }
        IconKind::Questionmark => {
            let raw: ImageRawLE<Rgb565> = ImageRaw::new(&questionmark_icon_file, 24);
            let image = Image::new(&raw, point);
            image.draw(display).unwrap();
        }
    }
}
fn undraw_icon(point: Point, display: &mut FramebufferDisplay) {
    let style = PrimitiveStyleBuilder::new()
        .fill_color(Rgb565::CSS_DARK_GRAY)
        .build();

    Rectangle::new(point, Size::new(24, 20))
        .into_styled(style)
        .draw(display)
        .unwrap();
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
fn undraw_text(fb: &mut [u8], width: usize, height: usize, msg: &str, point: Point, is_selected: bool) {
    let mut display = FramebufferDisplay { buf: fb, width, height };
    if is_selected {
        let style = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_DARK_GRAY);

        Text::with_baseline(msg, point, style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
    else {
        let style = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_DARK_GRAY);

        Text::with_baseline(msg, point, style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
    }
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
fn format_dir(current_dir: PathBuf) -> String {
    let string = current_dir.to_str().unwrap().to_owned();
    let mut formatted = string.replace("/yassin", "");
    if formatted.len() > 40 {
        formatted.replace_range(40..=formatted.len() - 1, "...");
        formatted
    }
    else {
        formatted
    }
}
async fn start_drawing_task(mut draw_rx: mpsc::Receiver<DrawCommand>) {
    tokio::task::spawn_blocking(move || {
        // spi
        let fb = Framebuffer::new("/dev/fb1").expect("Failed to open framebuffer");
        let width = fb.get_size().0 as usize;
        let height = fb.get_size().1 as usize;
        let bpp = fb.get_bytes_per_pixel() as usize;

        let mut mapped = fb.map().expect("Failed to map framebuffer memory");



        // two i2c screens, initialize and default draws - not storing values after shutdown atm
        let i2c_screen1_dev = linux_embedded_hal::I2cdev::new("/dev/i2c-1").unwrap();
        let i2c_screen2_dev = linux_embedded_hal::I2cdev::new("/dev/i2c-2").unwrap();

        let i2c_screen1_interface = I2CDisplayInterface::new(i2c_screen1_dev);
        let mut i2c_screen1_display = Ssd1306::new(i2c_screen1_interface, DisplaySize128x32, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        i2c_screen1_display.init().unwrap();
        i2c_screen1_display.clear_buffer();
        i2c_screen1_display.flush().unwrap();

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

        Text::with_baseline("Navigating", draw::DISPLAYSTATE_COORDS, text_style, Baseline::Top)
            .draw(&mut i2c_screen1_display)
            .unwrap();
        i2c_screen1_display.flush().unwrap();

        Text::with_baseline("0:00 / 0:00", draw::TOP_MEDIA_TIMESTAMP_COORDS, text_style, Baseline::Top)
            .draw(&mut i2c_screen2_display)
            .unwrap();
        i2c_screen2_display.flush().unwrap();

        Text::with_baseline(format!("Volume: 0%").as_str(), Point::new(0, 20), text_style, Baseline::Top)
            .draw(&mut i2c_screen2_display)
            .unwrap();
        i2c_screen2_display.flush().unwrap();

        while let Some(cmd) = draw_rx.blocking_recv() {
            match cmd {
                DrawCommand::Text { content, position, undraw, is_selected } => {
                    if undraw {
                        undraw_text(&mut mapped, width, height, content.as_str(), position, is_selected);
                    }
                    else {
                        draw_text(&mut mapped, width, height, content.as_str(), position);
                    }
                },
                DrawCommand::ConfirmingBackground { message, options } => {
                    draw_modal(&mut mapped, width, height, &message, options);
                },
        // current dir, 
                DrawCommand::NavigatingBackground { current_dir, file_count, current_index } => {
                    draw_nav_background(&mut mapped, width, height, current_dir, file_count, current_index);
                }
                DrawCommand::RawFrame { data } => {
                    draw_raw_frame(&mut mapped, &data);
                },
                DrawCommand::ClearScreen => {
                    clear_screen(&mut mapped);
                }
                DrawCommand::DrawI2CText { content, position, undraw, screen } => {
                    if screen == false {
                        if undraw {
                            undraw_i2c_text(&mut i2c_screen1_display, content.as_str(), position);
                        }
                        else {
                            draw_i2c_text(&mut i2c_screen1_display, content.as_str(), position);
                        }
                    }
                    else {
                        if undraw {
                            undraw_i2c_text(&mut i2c_screen2_display, content.as_str(), position);
                        }
                        else {
                            draw_i2c_text(&mut i2c_screen2_display, content.as_str(), position);
                        }
                    }
                }
                DrawCommand::ClearI2CScreen (screen) => {
                    // screen 1
                    if screen == false {
                        clear_i2c_screen(&mut i2c_screen1_display);
                    }
                    // screen 2
                    else {
                        clear_i2c_screen(&mut i2c_screen2_display);
                    }
                }
                DrawCommand::SelectNo => {
                    select_no(&mut mapped, width, height);
                }
                DrawCommand::SelectYes => {
                    select_yes(&mut mapped, width, height);
                }
                DrawCommand::DrawIcon { point, kind, undraw } => {
                    let mut display = FramebufferDisplay { buf: &mut mapped, width, height };
                    if undraw {
                        undraw_icon(point, &mut display);
                    }
                    else {
                        draw_icon(point, &mut display, kind);
                    }
                }
                _ => ()
                // DrawCommand::DrawI2CText { content, position, undraw } => {
                //     if undraw {
                //         undraw_text(&mut mapped, width, height, content.as_str(), position);
                //     }
                //     else {
                //         draw_text(&mut mapped, width, height, content.as_str(), position);
                //     }
                // }
            }
        }
    });
}

fn draw_i2c_text(
    display: &mut Ssd1306<I2CInterface<I2cdev>, DisplaySize128x32, BufferedGraphicsMode<DisplaySize128x32>>,
    content: &str,
    point: Point
) {
    let style = MonoTextStyleBuilder::new()
        .font(&FONT_8X13)
        .text_color(BinaryColor::On)
        .build();

    Text::with_baseline(content, point, style, Baseline::Top)
        .draw(display)
        .unwrap();
    display.flush().unwrap();
}
fn undraw_i2c_text(
    display: &mut Ssd1306<I2CInterface<I2cdev>, DisplaySize128x32, BufferedGraphicsMode<DisplaySize128x32>>,
    content: &str,
    point: Point,
) {
    let style = MonoTextStyleBuilder::new()
        .font(&FONT_8X13)
        .text_color(BinaryColor::Off)
        .build();

    Text::with_baseline(content, point, style, Baseline::Top)
        .draw(display)
        .unwrap();
    display.flush().unwrap();
}
fn clear_i2c_screen(display: &mut Ssd1306<I2CInterface<I2cdev>, DisplaySize128x32, BufferedGraphicsMode<DisplaySize128x32>>) {
    display.clear_buffer();
    display.flush().unwrap();
}


// do nothing len 0/1
// determine where in iteration u are, so that u can undraw and draw if there is index-1, and index+1/index+2, or vice versa
// can animate these in future
async fn scroll_up(nav_state: &NavigatingData, draw_tx: mpsc::Sender<DrawCommand>) {

    let readdir: Vec<_> = std::fs::read_dir(nav_state.current_dir.to_owned()).unwrap().collect::<Result<_, _>>().unwrap();
    let idx_plus_one = readdir.get(nav_state.current_index + 1);
    let current_idx = readdir.get(nav_state.current_index);
    let idx_minus_one = { if nav_state.current_index == 0 { None } else { readdir.get(nav_state.current_index - 1) } };
    let idx_minus_two = { if nav_state.current_index == 0 || nav_state.current_index == 1 { None } else { readdir.get(nav_state.current_index - 2) } };

    // undraw based on indexes available
    if let Some(idx_minus_one) = idx_minus_one {
        let kind = determine_icon_to_draw(idx_minus_one);
        draw_tx.send(DrawCommand::DrawIcon { point: TOP_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_minus_one.file_name().to_str().unwrap().to_owned(), position: draw::TOP_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }
    if let Some(current_idx) = current_idx {
        let kind = determine_icon_to_draw(current_idx);
        let mtime = current_idx.metadata().unwrap().mtime();
        let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
        let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
        // file size, 
        draw_tx.send(DrawCommand::Text { content: utils::format_bytes(current_idx.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
        // last modified
        draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
        // icon
        draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        // file name
        draw_tx.send(DrawCommand::Text { content: current_idx.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: true, is_selected: true,}).await.unwrap();
    }
    if let Some(idx_plus_one) = idx_plus_one {
        let kind = determine_icon_to_draw(idx_plus_one);
        draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_plus_one.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }

    // draw indexes based on new upcoming states
    if let Some(current_idx) = current_idx {
        let kind = determine_icon_to_draw(current_idx);
        draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: current_idx.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: false, is_selected: true,}).await.unwrap();
    }
    if let Some(idx_minus_one) = idx_minus_one {
        let kind = determine_icon_to_draw(idx_minus_one);
        let mtime = idx_minus_one.metadata().unwrap().mtime();
        let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
        let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
        // file size, 
        draw_tx.send(DrawCommand::Text { content: utils::format_bytes(idx_minus_one.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
        // last modified
        draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
        draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_minus_one.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    }
    if let Some(idx_minus_two) = idx_minus_two {
        let kind = determine_icon_to_draw(idx_minus_two);
        draw_tx.send(DrawCommand::DrawIcon { point: TOP_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_minus_two.file_name().to_str().unwrap().to_owned(), position: draw::TOP_CAROUSEL_TXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    }

    // undraw and draw the new current index
    // draw_tx.send(DrawCommand::Text { content: format!("1/{}", file_count), position: Point::new(46, 18), undraw: false, is_selected: false }).await.unwrap();
    draw_tx.send(DrawCommand::Text { content: format!("{}/{}", nav_state.current_index + 1, nav_state.file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    draw_tx.send(DrawCommand::Text { content: format!("{}/{}", nav_state.current_index, nav_state.file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: false, is_selected: false,}).await.unwrap();
}
async fn scroll_down(nav_state: &NavigatingData, draw_tx: mpsc::Sender<DrawCommand>) {
    let readdir: Vec<_> = std::fs::read_dir(nav_state.current_dir.to_owned()).unwrap().collect::<Result<_, _>>().unwrap();
    let idx_minus_one = { if nav_state.current_index == 0 { None } else { readdir.get(nav_state.current_index - 1) } };
    let current_idx = readdir.get(nav_state.current_index);
    let idx_plus_one = readdir.get(nav_state.current_index + 1);
    let idx_plus_two = readdir.get(nav_state.current_index + 2);

    // undraw based on indexes available
    if let Some(idx_plus_one) = idx_plus_one {
        let kind = determine_icon_to_draw(idx_plus_one);
        draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_plus_one.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }
    if let Some(current_idx) = current_idx {
        let kind = determine_icon_to_draw(current_idx);
        let mtime = current_idx.metadata().unwrap().mtime();
        let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
        let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
        // file size, 
        draw_tx.send(DrawCommand::Text { content: utils::format_bytes(current_idx.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
        // last modified
        draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
        draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: current_idx.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: true, is_selected: true,}).await.unwrap();
    }
    if let Some(idx_minus_one) = idx_minus_one {
        let kind = determine_icon_to_draw(idx_minus_one);
        draw_tx.send(DrawCommand::DrawIcon { point: TOP_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_minus_one.file_name().to_str().unwrap().to_owned(), position: draw::TOP_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }

    // draw indexes based on new upcoming states
    if let Some(current_idx) = current_idx {
        let kind = determine_icon_to_draw(current_idx);
        draw_tx.send(DrawCommand::DrawIcon { point: TOP_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: current_idx.file_name().to_str().unwrap().to_owned(), position: draw::TOP_CAROUSEL_TXT_COORDS, undraw: false, is_selected: true,}).await.unwrap();
    }
    if let Some(idx_plus_one) = idx_plus_one {
        let kind = determine_icon_to_draw(idx_plus_one);
        let mtime = idx_plus_one.metadata().unwrap().mtime();
        let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
        let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
        // file size, 
        draw_tx.send(DrawCommand::Text { content: utils::format_bytes(idx_plus_one.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
        // last modified
        draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
        draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_plus_one.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    }
    if let Some(idx_plus_two) = idx_plus_two {
        let kind = determine_icon_to_draw(idx_plus_two);
        draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_plus_two.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    }
    draw_tx.send(DrawCommand::Text { content: format!("{}/{}", nav_state.current_index + 1, nav_state.file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    draw_tx.send(DrawCommand::Text { content: format!("{}/{}", nav_state.current_index + 2, nav_state.file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: false, is_selected: false,}).await.unwrap();
}
enum SelectResponse {
    // (file type, file size, file name)
    File {
        file_name: String,
        file_size: u64,
        file_extension: String,
        file_path: PathBuf,
        last_modified: String,
    },
    Directory{
        file_path: PathBuf,
        file_count: usize,
        last_modified: String,
    },
    Error(String),
    FatalError(String),
}

async fn enter_dir_or_select_file(nav_state: &NavigatingData, draw_tx: mpsc::Sender<DrawCommand>) -> SelectResponse {
    let readdir: Vec<_> = std::fs::read_dir(nav_state.current_dir.to_owned()).unwrap().collect::<Result<_, _>>().unwrap();
    let idx_plus_one = readdir.get(nav_state.current_index + 1);
    let current_idx = readdir.get(nav_state.current_index);
    let idx_minus_one = { if nav_state.current_index == 0 { None } else { readdir.get(nav_state.current_index - 1) } };

    if let Some(entry) = readdir.get(nav_state.current_index) {
        if let Ok(meta) = entry.metadata() {
            if meta.is_dir() {
                let new_dir: Vec<_> = std::fs::read_dir(entry.path()).unwrap().collect::<Result<_, _>>().unwrap();
                // undraw the current current_index/file_count,
                draw_tx.send(DrawCommand::Text { content: format!("{}/{}", nav_state.current_index + 1, nav_state.file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: true, is_selected: false,}).await.unwrap();
                draw_tx.send(DrawCommand::Text { content: format_dir(nav_state.current_dir.to_owned()), position: draw::TOP_NAV_PATH_COORDS, undraw: true, is_selected: false,}).await.unwrap();

                // undraw possible idx_plus_one, current_idx, and idx_minus_one
                if let Some(idx_plus_one) = idx_plus_one {
                    let kind = determine_icon_to_draw(idx_plus_one);
                    draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
                    draw_tx.send(DrawCommand::Text { content: idx_plus_one.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
                }
                if let Some(current_idx) = current_idx {
                    let kind = determine_icon_to_draw(current_idx);
                    let mtime = current_idx.metadata().unwrap().mtime();
                    let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
                    let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
                    // file size, 
                    draw_tx.send(DrawCommand::Text { content: utils::format_bytes(current_idx.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
                    // last modified
                    draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
                    draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
                    draw_tx.send(DrawCommand::Text { content: current_idx.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: true, is_selected: true,}).await.unwrap();
                }
                if let Some(idx_minus_one) = idx_minus_one {
                    let kind = determine_icon_to_draw(idx_minus_one);
                    draw_tx.send(DrawCommand::DrawIcon { point: TOP_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
                    draw_tx.send(DrawCommand::Text { content: idx_minus_one.file_name().to_str().unwrap().to_owned(), position: draw::TOP_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
                }

                // new path is the one u selected, draw their stuff if exists
                let file_count = new_dir.iter().count();
                let new_current_idx = new_dir.get(0);
                let new_idx_plus_one = new_dir.get(1);

                if file_count == 0 {
                    draw_tx.send(DrawCommand::Text { content: format!("{}/{}", 0, file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: false, is_selected: false,}).await.unwrap();
                }
                else {
                    draw_tx.send(DrawCommand::Text { content: format!("{}/{}", 1, file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: false, is_selected: false,}).await.unwrap();
                }
                if let Some(new_idx_plus_one) = new_idx_plus_one {
                    let kind = determine_icon_to_draw(new_idx_plus_one);
                    draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
                    draw_tx.send(DrawCommand::Text { content: new_idx_plus_one.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();
                }
                if let Some(new_current_idx) = new_current_idx {
                    let kind = determine_icon_to_draw(new_current_idx);
                    let mtime = new_current_idx.metadata().unwrap().mtime();
                    let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
                    let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
                    // file size, 
                    draw_tx.send(DrawCommand::Text { content: utils::format_bytes(new_current_idx.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
                    // last modified
                    draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
                    draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
                    draw_tx.send(DrawCommand::Text { content: new_current_idx.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: false, is_selected: true,}).await.unwrap();
                }
                draw_tx.send(DrawCommand::Text { content: format_dir(entry.path().to_owned()), position: draw::TOP_NAV_PATH_COORDS, undraw: false, is_selected: false,}).await.unwrap();
                let mtime = entry.metadata().unwrap().mtime();
                let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
                let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
                SelectResponse::Directory { file_path: entry.path(), file_count: file_count, last_modified } 
            }
            else if meta.is_file() {
                // check extension
                // if .raw or .rgb565, prompt to play

                // for now, just play
                println!("This is a file!");
                let path = entry.path();
                let file_extension = path.extension();
                if let Some(file_extension) = file_extension {
                    let mtime = entry.metadata().unwrap().mtime();
                    let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
                    let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
                    SelectResponse::File { file_name: entry.file_name().to_str().unwrap().to_owned(), file_size: meta.len(), file_extension: file_extension.to_str().unwrap().to_owned(), file_path: path, last_modified }
                }
                else {
                    SelectResponse::Error(String::from("Filetype error: File can not be opened, must be either a .mp3, rgb565, or .raw file."))
                }
            }
            else {
                SelectResponse::Error(String::from("Filetype error: File can not be opened: must be either a .mp3, rgb565, or .raw file."))
            }
        }
        else {
                SelectResponse::Error(String::from("Unknown error: File could not be opened."))
        }
    }
    else {
        SelectResponse::Error(String::from("There are no files or directories in this path."))
    }
}

async fn exit_dir(nav_state: &NavigatingData, draw_tx: mpsc::Sender<DrawCommand>) -> Option<(PathBuf, usize)> {
    let readdir: Vec<_> = std::fs::read_dir(nav_state.current_dir.to_owned()).unwrap().collect::<Result<_, _>>().unwrap();
    let idx_plus_one = readdir.get(nav_state.current_index + 1);
    let current_idx = readdir.get(nav_state.current_index);
    let idx_minus_one = { if nav_state.current_index == 0 { None } else { readdir.get(nav_state.current_index - 1) } };

    // undraw the current current_index/file_count,
    if nav_state.file_count == 0 {
        draw_tx.send(DrawCommand::Text { content: format!("{}/{}", 0, nav_state.file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }
    else {
        draw_tx.send(DrawCommand::Text { content: format!("{}/{}", nav_state.current_index + 1, nav_state.file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }
    draw_tx.send(DrawCommand::Text { content: format_dir(nav_state.current_dir.to_owned()), position: draw::TOP_NAV_PATH_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    // if there are files or dirs, undraw them
    if let Some(idx_plus_one) = idx_plus_one {
        let kind = determine_icon_to_draw(idx_plus_one);
        draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_plus_one.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }
    if let Some(current_idx) = current_idx {
        let kind = determine_icon_to_draw(current_idx);
        let mtime = current_idx.metadata().unwrap().mtime();
        let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
        let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
        // file size, 
        draw_tx.send(DrawCommand::Text { content: utils::format_bytes(current_idx.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
        // last modified
        draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: true, is_selected: true }).await.unwrap();
        draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: current_idx.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: true, is_selected: true,}).await.unwrap();
    }
    if let Some(idx_minus_one) = idx_minus_one {
        let kind = determine_icon_to_draw(idx_minus_one);
        draw_tx.send(DrawCommand::DrawIcon { point: TOP_CAROUSEL_ICON_COORDS, kind, undraw: true }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: idx_minus_one.file_name().to_str().unwrap().to_owned(), position: draw::TOP_CAROUSEL_TXT_COORDS, undraw: true, is_selected: false,}).await.unwrap();
    }

    // go up one in current directory
    let mut new_path = nav_state.current_dir.to_owned();
    new_path.pop();
    let new_dir: Vec<_> = std::fs::read_dir(new_path.to_owned()).unwrap().collect::<Result<_, _>>().unwrap();
    println!("new directory: {:#?}", new_dir);
    let file_count = new_dir.iter().count();
    let new_current_idx = new_dir.get(0);
    let new_idx_plus_one = new_dir.get(1);

    if file_count == 0 {
        draw_tx.send(DrawCommand::Text { content: format!("{}/{}", 0, file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    }
    else {
        draw_tx.send(DrawCommand::Text { content: format!("{}/{}", 1, file_count), position: draw::TOP_NAV_FILE_INDEX_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    }
    if let Some(new_idx_plus_one) = new_idx_plus_one {
        let kind = determine_icon_to_draw(new_idx_plus_one);
        draw_tx.send(DrawCommand::DrawIcon { point: BOTTOM_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: new_idx_plus_one.file_name().to_str().unwrap().to_owned(), position: draw::BOTTOM_CAROUSEL_TXT_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    }
    if let Some(new_current_idx) = new_current_idx {
        let kind = determine_icon_to_draw(new_current_idx);
        let mtime = new_current_idx.metadata().unwrap().mtime();
        let unformatted_last_modified = DateTime::from_timestamp(mtime, 0).unwrap().naive_local();
        let last_modified = unformatted_last_modified.format("%m-%d-%Y, %-I:%M%P").to_string();
        // file size, 
        draw_tx.send(DrawCommand::Text { content: utils::format_bytes(new_current_idx.metadata().unwrap().size()), position: ENTRY_META_FILESIZE_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
        // last modified
        draw_tx.send(DrawCommand::Text { content: last_modified, position: draw::ENTRY_META_LASTMODIFIED_TEXT_COORDS, undraw: false, is_selected: true }).await.unwrap();
        draw_tx.send(DrawCommand::DrawIcon { point: MIDDLE_CAROUSEL_ICON_COORDS, kind, undraw: false }).await.unwrap();
        draw_tx.send(DrawCommand::Text { content: new_current_idx.file_name().to_str().unwrap().to_owned(), position: draw::MIDDLE_CAROUSEL_TXT_COORDS, undraw: false, is_selected: true,}).await.unwrap();
    }
    draw_tx.send(DrawCommand::Text { content: format_dir(new_path.to_owned()), position: draw::TOP_NAV_PATH_COORDS, undraw: false, is_selected: false,}).await.unwrap();
    Some((new_path, file_count))
}

async fn play_video(current_frame: Arc<AtomicU64>, paused: Arc<AtomicBool>, file_details: Option<FileDetails>, draw_tx: mpsc::Sender<DrawCommand>) {
    if let Some(file_details) = file_details {
        // 2 bytes per pixel btw
        let frame_bytes = WIDTH as usize * HEIGHT as usize * 2;

        // 24 fps
        let frame_delay = Duration::from_millis(42);

        let start_from = frame_bytes * current_frame.load(Ordering::Relaxed) as usize;

        // open bgr565le file
        // start from current frame, so dball would be with an offset
        let mut video_file = File::open(file_details.file_path).unwrap();
        let mut frame = vec![0u8; frame_bytes];
        video_file.seek(std::io::SeekFrom::Start(start_from as u64)).unwrap();

        // start from current frame
        while let Ok(()) = video_file.read_exact(&mut frame) {
            // break if paused
            if paused.load(Ordering::Relaxed) == true {
                break;
            }
            draw_tx.send(DrawCommand::RawFrame { data: frame.clone() }).await.unwrap();
            tokio::time::sleep(frame_delay).await;
            current_frame.fetch_add(1, Ordering::Relaxed);
        }
    }
}


