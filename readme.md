general information for this binary

build info:

    requires clangd of at least vers. 12.0 I'm pretty sure in order to build.

    target: aarch64-unknown-linux-gnu

    cross build --target aarch64-unknown-linux-gnu

    then can use on raspberry pi zero 2 w

pi info:

    set the 4 gpios from the bottom left after being orientated on the pinout page to be input pullup
    set the 2 gpios from the bottom right to be input pullup as well

    connect spi, (where dc= gpio22, and reset = gpio27, led = 5 I think) and two i2c devices
    create systemctl service to run on boot that'll execute binary built from above



