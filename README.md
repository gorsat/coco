## <span>Rusty CoCo <br> TRS-80 Color Computer 1 & 2 emulator written in Rust</span>

<img align="right" width="400" alt="Rusty CoCo running hello.asm on a Mac" src="https://user-images.githubusercontent.com/10043170/223582721-8417ef45-73fa-4f3f-8234-87c406eefc83.png">

Rusty CoCo emulates the color computer's hardware on Mac, Windows and Linux. 
Graphics, sound, keyboard and joystick (using mouse) are all supported. 
Peripherals like cassette, disk and RS-232 are not supported (_yet?_). 
It can run basic and extended basic and every cartridge I've tried.

I undertook this project to improve my knowledge of Rust while also reliving some of my earliest computing experiences. 
Writing Basic programs with sound and graphics 
and playing old games that I last played 40 years ago has been fun, but I think the pinnacle for me was loading up the 
EDTASM+ cartridge and being instantly transported back to my first exposure to 
assembly language as a kid (with Lance Leventhal's book in hand). 
It felt magical back then. 
Still does.
<br clear="left"/>  
## Features

This project builds on my [6809 simulator](https://github.com/gorsat/6809) project. 
As such, many features of that program are included in this one like rich debugging and the 
ability to directly load and run 6809 assembly language code rather than just binaries. 
The picture above shows the emulator running the included [hello.asm](/hello.asm) program.

The coco emulator is not cycle accurate and, because the CPU is not running on a realtime thread, it's possible to experience some inconsistent timing behavior. 
This should be pretty innocuous unless you're on a severely resource constrained machine. 
The emulator uses minifb to render video. 
Minifb is run on its own thread to try to isolate it from the CPU simulator. 
However, on a Mac I've seen the video rendering call into minifb cause the CPU thread to stall for 10's of microseconds. 
Typically, this isn't a real problem, but it can make coco audio a bit rougher sounding.
For instance, in basic you can type ```PLAY "O2C"``` to play the note C in the 2nd octave. 
If this note sounds a little rough then that's due to these timing issues.

## ROMs and Getting Started
The color computer's operating system resides in two different ROM images, one that's just called "Basic" and another that's typically called "Extended Basic". 
To load these ROMs you need to add their paths to the [coco.yaml](/coco.yaml) file along with the addresses at which they should be loaded. 
[coco.yaml](/coco.yaml) should reside in the working directory where you run coco. 

I'm not including the ROMs here in this repository because of copyright concerns, but they're easy to find elsewhere. 
I recommend [Color Computer Archive](https://colorcomputerarchive.com/) as a great source for ROMs, cartridges, manuals and much more. 

The [coco.yaml](/coco.yaml) file checked into the repo looks like this:
```yaml
load_rom:
  # - path: "BASIC.ROM"
  #   addr: 0xa000
  # - path: "EXTBASIC.ROM"
  #   addr: 0x8000
load_code:
  - path: "hello.asm"
```
As you can see, the lines under ```load_rom``` are all commented out, so this file is setup to _not_ load any ROMs. 
Instead, it just loads "hello.asm". 
This is a little demo program that lets you quickly try the emulator by simply cloning the repo and typing ```cargo run```. When you do that, you should see a window that looks just like the picture above.

> _NOTE for Linux_ -- You may encounter an error when building on linux due to missing ALSA file(s).
> The fix for this on Debian derivatives (e.g., Ubuntu, Pop!) is to install libasound2-dev.
> On my Pop! box I did this with ```sudo apt install libasound2-dev```. For other flavors of 
> linux you'll have to search up the solution.

In order to make coco behave like a real color computer, you'll have to comment out the  ``` - path: "hello.asm"``` line and then uncomment the lines under ```load_rom```. 
These lines tell the emulator where to find the ROM files and the addresses in memory at which they should be loaded. 

Of course, you'll have to download the ROM files first. 
The ROMs I'm using can be found [here](https://colorcomputerarchive.com/repo/ROMs/David%20Keil/coco-2/).
Once you've altered [coco.yaml](/coco.yaml) and placed the ROM files in the working directory, executing ```cargo run``` should launch the emulator right into the startup screen of the original color computer. 

## Cartridges, Code and Load Order
Loading cartridges can be accomplished via the command line using ```--cart <path_to_cart_file>```. 
Cartridge files (file extensions might be .ccc or .bin) are just binary files containing raw 6809 machine language. 
When you tell the emulator that a binary file is actually cartridge, the file is loaded starting at 0xC000 in the 6809's address space and a FIRQ interrupt is raised. 
And if all you want to do is run some games on cartridges then you may be wondering if you need to care about the OS ROMs at all. 
Cartridges depend on code in the ROMs both directly and indirectly. 
- Some -- but not all -- programs on cartridges rely on functions in the ROM. 
- Almost all cartridges I've seen depend on the ROM to initialize the stack and they'll just crash and burn if this isn't done before they're run. 
- The ROM handles the loading and execution of cartridges. 
All this entails is handling the FIRQ and then jumping to 0xC000 (the address where cartridges are loaded). 
I considered having the emulator initialize the stack and launch the cartridge code when a cartridge is present but decided against it after learning that some cartridges actually depend on code in ROM.

So, yes, you have to get the ROMs and load them in order to run cartridges.

### Load Order
The emulator can load ROMs, cartridges and arbitrary code (asm or hex files). 
These are loaded (but not run) in the following order:

1. Cartridge
2. ROMs
3. Code listed in coco.yaml
4. Code referenced with --load

This allows you to use your own code to patch ROMs or cartridges. There's an example of such a patch in [disable_wait_routine.asm](/disable_wait_routine.asm) which circumvents one of the wait loops in Basic. I have used this to speed up debugging (because that wait loop takes several seconds to execute when the debugger is enabled). 
If you want to generate .hex files then you can use the [6809](https://gorsat.github.com/6809) project, but there's really no need since coco will build and run .asm files directly.






### Options
You can run the program with the ```--help``` (or ```-h```) option to see all the available options. 
Note that many of the options are holdovers from the 6809 project. 
I honestly haven't tried many of these and some don't really even make sense in the new program, so beware.
Perhaps at some point I'll clean up all the unnecessary options.

Here are some useful command line options for the coco emulator:
```
      --load <LOAD>
          Assembly (.asm, .s) or Hex (.hex) file to assemble/run/debug
  -b, --break-start
          Break into the debugger before running the program (only if debugger enabled)
      --cart <CART>
          Load a cartridge from file
  -d, --debug
          Run with debugger enabled
  -m, --mhz <MHZ>
          Limits the clock speed in MHz (default is unlimited)
      --perf
          Display perf data (only interesting for longer-running programs)
  -t, --time <TIME>
          Set the duration in seconds for which the program should run
```
### --mhz
The most important of these is the ```--mhz``` option. This lets you limit the speed of the 6809 emulator. If you're playing a game or playing music or anything else for which the speed of the CPU matters, then you'll want to use this option and set it to something like ```-m 0.9```. 
This does _NOT_ guarantee that the emulator will run at an effective clock speed of 0.9 MHz. 
It simply limits the execution speed such that the emulator's effective clock speed will be _no higher than_ 0.9 MHz. 
### --perf
Note that when using ```--perf``` the performance data is only displayed once the emulator exits so you'll typically want to use 
this option with the ```--time``` option to set a finite duration for the program. 
You can use the ```--perf``` option to see what the emulator's effective clock speed is on your system. 
Type the following command:
```
cargo r -r -- --perf --time 5
```
This will run the retail build of coco for 5 seconds and then produce output something like this:
```
INFO: Executed 10869484 instructions in 5.00 sec; 2.174 MIPS; effective clock: 7.640 MHz
```
In this example I'm running coco with the Basic and Extended Basic ROMs loaded on an old i5 Mac mini.
Performance is measured using Instant and Duration and it's highly dependent on what the code is actually doing. 
So if you're really looking for accuracy, then don't look here :-).
### --debug
The ```--debug``` option turns on the debugger.
This slows execution substantially because every instruction is disassembled and saved in a running history, so only use it if you need it (or if you want to check out some of that sweet, sweet 6809 code). 
The ```--break-start``` option only makes sense in conjunction with the ```--debug``` option. 
Typically I use the short flags ```-db``` to start coco at the debug prompt. 
Once you're in the debugger, you can just type ```h``` to get help with all the available commands.
