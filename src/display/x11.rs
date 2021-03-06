extern crate x11;

use super::{Screen};
use super::base::*;

use std::{ffi, ptr};
use self::x11::xlib::*;

pub struct XScreen {
    display: *mut Display,
    screen: i32,
    win: u64,
    white: u64,
    black: u64,
    cmap: Colormap,
    scale: u64
}

impl XScreen {
    pub fn new(scale: u64) -> XScreen {
        println!("Starting new XScreen...");

        unsafe {
            Self::open_screen(scale)
        }
    }

    // TODO: There is like no error handling code as part of the C
    // FFI. That should be added.
    unsafe fn open_screen(scale: u64) -> XScreen {
        let x_none = 0;
        let display = XOpenDisplay(ptr::null());
        let screen = XDefaultScreen(display);
        let (black, white) = (XBlackPixel(display, screen),
                              XWhitePixel(display, screen));
        let cmap = XDefaultColormap(display, screen);
        let win = XCreateSimpleWindow(display, XDefaultRootWindow(display),
                                      /*x=*/0, /*y=*/0, /*width=*/128, /*height=*/160,
                                      /*border_width=*/0, /*border=*/black, /*background=*/black);
        let win_title = ffi::CString::new("Ajidamal Simulator").unwrap();
        let win_icon = ffi::CString::new("aji/sim").unwrap();
        XSetStandardProperties(display, win, win_title.as_ptr(), win_icon.as_ptr(),
                               /*pixmap=*/x_none, /*argv=*/ptr::null_mut(), /*argc=*/0,
                               /*hints=*/ptr::null_mut());
        XClearWindow(display, win);
        XMapRaised(display, win);

        let mut screen = XScreen {
            display: display,
            screen: screen,
            win: win,
            white: white,
            black: black,
            cmap: cmap,
            scale: scale
        };

        screen.flush();

        screen
    }
}

impl Screen for XScreen {
    fn dimensions(&self) -> (u32, u32) {
        (128, 160)
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: Color) {
        let (r, g, b) = color.intensities();

        unsafe {
            let mut xcolor = XColor {
                pixel: 0,
                // Convert 8-bit color to 16-bit color
                red: (r << 8) as u16,
                green: (g << 8) as u16,
                blue: (b << 8) as u16,
                flags: 0,
                pad: 0,
            };
            let status = XAllocColor(self.display, self.cmap, &mut xcolor);
            if status == 0 {
                panic!("Color allocation failed.");
            }

            let gc = XCreateGC(self.display, self.win, 0, ptr::null_mut());
            XSetBackground(self.display, gc, xcolor.pixel);
            XSetForeground(self.display, gc, xcolor.pixel);

            XFillRectangle(self.display, self.win, gc,
                           (x as u64 * self.scale) as i32, (y as u64 * self.scale) as i32,
                           self.scale as u32, self.scale as u32);
            XFreeGC(self.display, gc);
        }
    }

    fn flush(&mut self) {
        unsafe {
            XFlush(self.display);
        }
    }
}
