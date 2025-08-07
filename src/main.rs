use embedded_graphics::{
    mono_font::{ascii::{FONT_6X10, FONT_8X13}, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use embedded_hal::digital::{InputPin, OutputPin};
use linux_embedded_hal::gpio_cdev::{Chip, EventRequestFlags, EventType, LineRequestFlags};
use linux_embedded_hal::i2cdev::core::I2CDevice;
use linux_embedded_hal::{ CdevPin };
use std::io::prelude::*;
use std::fs::File;
use std::thread;
use std::time::{Duration, Instant};
use linuxfb::Framebuffer;
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
// const WIDTH: usize = 320;
// const HEIGHT: usize = 240;
// const FRAME_SIZE: usize = WIDTH * HEIGHT * 2;
const SSD1306_SLAVE_ADDR: u16 = 0x3c;

fn main() -> ! {
    println!("Hello, world!");
    // gpio setup
    let mut chip = Chip::new("/dev/gpiochip0").unwrap();

    // i2c
    let i2c_screen1_dev = linux_embedded_hal::I2cdev::new("/dev/i2c-1").unwrap();
    let i2c_screen2_dev = linux_embedded_hal::I2cdev::new("/dev/i2c-2").unwrap();

    // init sequence -  not needed cause not using i2cdev::I2cDevice or whatev, using just I2cDev::new
    // i2c_screen1_dev.smbus_write_byte_data(0xF0, 0x55).unwrap();
    // i2c_screen1_dev.smbus_write_byte_data(0xFB, 0x00).unwrap();
    // i2c_screen2_dev.smbus_write_byte_data(0xF0, 0x55).unwrap();
    // i2c_screen2_dev.smbus_write_byte_data(0xFB, 0x00).unwrap();
    // thread::sleep(Duration::from_millis(100));



    // screen1
    let i2c_screen1_interface = I2CDisplayInterface::new(i2c_screen1_dev);
    let mut i2c_screen1_display = Ssd1306::new(i2c_screen1_interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    i2c_screen1_display.init().unwrap();
    i2c_screen1_display.clear_buffer();
    i2c_screen1_display.flush().unwrap();

    //screen2
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
    backlight.set_high().unwrap(); // enable display backlight

    // open framebuffer device, get properties
    let fb = Framebuffer::new("/dev/fb1").unwrap();
    let (width, height) = fb.get_size();
    let bpp = fb.get_bytes_per_pixel() as usize;
    let (vx, vy) = fb.get_virtual_size();
    eprintln!("fb: {}×{}, virtual {}×{}", width, height, vx, vy);

    // map framebuffer mem once
    let mut fb_mem = fb.map().unwrap();

    // bytes per file & and fps
    let frame_bytes = width as usize * height as usize * bpp;
    // 24 fps
    let frame_delay = Duration::from_millis(42);

    // open rgb565 file
    let mut dball = File::open("dragonball/goku_vs_piccolo_jr_le.raw").unwrap();
    let mut frame = vec![0u8; frame_bytes];

    println!("Stream: {} bytes/frame → {}ms/frame at 24fps", frame_bytes, 42);

    // stream loop

    // escape - black cable
    let mut escape_btn = CdevPin::new(chip.get_line(26).unwrap().request(LineRequestFlags::INPUT, 1, "esc").unwrap()).unwrap();

    // select - white cable
    // let mut select_btn = chip.get_line(19).unwrap().request(LineRequestFlags::INPUT, 1, "sel").unwrap();
    let select_btn_line = chip.get_line(19).unwrap();
    let select_btn_events = select_btn_line.events(
        LineRequestFlags::INPUT,
        EventRequestFlags::FALLING_EDGE,
        "sel"
    ).unwrap();

    // up - green cable
    let mut up_btn = CdevPin::new(chip.get_line(13).unwrap().request(LineRequestFlags::INPUT, 1, "up").unwrap()).unwrap();

    // down - beige cable
    let mut down_btn = CdevPin::new(chip.get_line(6).unwrap().request(LineRequestFlags::INPUT, 1, "dwn").unwrap()).unwrap();
    // let mut last_esc_btn_state = escape_btn.is_high().unwrap();
    // let mut last_sel_btn_state = select_btn.is_high().unwrap();
    // let mut last_up_btn_state = up_btn.is_high().unwrap();
    // let mut last_down_btn_state = down_btn.is_high().unwrap();

    // let esc_btn_state = escape_btn.is_high().unwrap();
    // let sel_btn_state = select_btn.is_high().unwrap();
    // let up_btn_state = up_btn.is_high().unwrap();
    // let down_btn_state = down_btn.is_high().unwrap();
    for event in select_btn_events {
        let evt = event.unwrap();
        match evt.event_type() {
            EventType::FallingEdge => {
                while let Ok(()) = dball.read_exact(&mut frame) {
                    let started = Instant::now();
                    fb_mem[..frame_bytes].copy_from_slice(&frame);

                    let elapsed = started.elapsed();
                    if elapsed < frame_delay {
                        thread::sleep(frame_delay - elapsed);
                    }
                }
            }
            _ => ()
        }
        // simple debounce
        thread::sleep(Duration::from_millis(50));
    }
    loop {}

    // For an ILI9341 with fbtft, expect width=240, height=320, bpp=2 and RGB565 layout
    // println!("Framebuffer: {}×{} at {} bytes/pixel", width, height, bpp);
    // println!("Format: {:?}", fb.get_pixel_layout());

    // double buffer, apparently driver supports automatic flipping.
    // let mut buf = linuxfb::double::Buffer::new(fb).unwrap();
    // let w = buf.width as usize;
    // let h = buf.height as usize;
    // let frame_bytes = w * h * bpp * 2;

    // let mut spi_dev = SpidevDevice::open("/dev/spidev0.0").unwrap();
    // let options = SpidevOptions::new()
    //     .bits_per_word(8)
    //     .max_speed_hz(40_000_000)
    //     .mode(SpiModeFlags::SPI_MODE_0)
    //     .build();
    // spi_dev.configure(&options).unwrap();

    // spi interface
    // let cs_output = CdevPin::new(chip.get_line(8).unwrap().request(LineRequestFlags::OUTPUT, 1, "cs_output").unwrap()).unwrap();
    // let spi_device = ExclusiveDevice::new(spi, cs_output, delay).unwrap();

    // let mut buffer = [0_u8; 1024];
    // let di = SpiInterface::new(spi_dev, dc, &mut buffer);
    //
    // //green led
    // let green_led = chip.get_line(2).unwrap().request(LineRequestFlags::OUTPUT, 0, "blinky").unwrap();
    //
    // let mut display = Builder::new(ILI9341Rgb565, di)
    //     .reset_pin(rst)
    //     // .orientation(Orientation { mirrored: true, rotation: Rotation::Deg270 })
    //     // .color_order(mipidsi::options::ColorOrder::Bgr)
    //     .init(&mut delay)
    //     .unwrap();
    // display.clear(Rgb565::BLACK).unwrap();

    //
    // loop {
    //     let start = Instant::now();
    //
    //
    //     // block until full frame is read
    //     if let Err(e) = dball.read_exact(&mut raw) {
    //         eprintln!("EOF or error reading frame: {:?}", e);
    //         loop {
    //         }
    //     }
    //
    //     // draw to back to buffer
    //     let back = buf.as_mut_slice();
    //     back.copy_from_slice(&raw);
    //
    //     // show frame
    //     buf.flip().unwrap();
    //
    //     // maintain timing
    //     let elapsed = start.elapsed();
    //     if elapsed < frame_delay {
    //         thread::sleep(frame_delay - elapsed);
    //     }
    //
    //     // let bytes_read = dball.read(frame_buffer).unwrap();
    //     //
    //     // let raw_frame: ImageRaw<Rgb565> = ImageRaw::<Rgb565>::new(frame_buffer, WIDTH as u32);
    //     // let frame = Image::new(&raw_frame, Point::zero());
    //     // frame.draw(&mut display).unwrap();
    //     //
    //     // if bytes_read < FRAME_SIZE {
    //     //     continue;
    //     // }
    //     // green_led.set_value(1).unwrap();
    //     // sleep(Duration::from_secs(1));
    //     // green_led.set_value(0).unwrap();
    //     // sleep(Duration::from_secs(1));
    // }
}
