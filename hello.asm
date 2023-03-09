sam     equ $ffc0 
pia0    equ $ff00
vram    equ $400

* arbitrarily starting our program somewhere
	org $2000
start:
* set the start of vram at $400 (this is what basic does) 
    sta sam+9 
* set the system stack pointer to somewhere valid
    lds #$1fff
* print the whole SG4 character set at the top of the screen
	clra
	ldx #vram
loop1:
	sta ,x+
	inca
	bne loop1
    lda #' '
    clrb
* clear the rest of the screen
loop2:
    sta ,x+
    incb
    bne loop2
* write our messages on the screen
    ldy #msg1
    ldx #vram+(10*32) ; NOTE: parens are necessary here unless you use the -p option!
    jsr strout
    ldy #msg2
    ldx #vram+(11*32)
    jsr strout
    ldy #msg3
    ldx #vram+(15*32)
    jsr strout
* wait for any keypresses
* first make a few settings in the "hardware" to enable keyboard matrix strobing
    lda #$ff
    sta pia0+2 ; set pia0-b-data as all output bits
    ldb #$34
    stb pia0+1
    stb pia0+3 ; deselect data direction registers
keys:
    clr pia0+2 ; strobe all keyboard columns
    lda pia0   ; get the results
    coma       ; flip the bits
    asla       ; ignore bit 7 (joystick comparitor) 
    beq keys   ; if no keys pressed then try again
end:
    exit       ; terminate the emulator

* function to print string at y to location at x
strout:
    lda ,y+
    beq so_end
    cmpa #$40
    blt so_putc
    anda #$1f
so_putc:
    sta ,x+
    bra strout
so_end:
    rts
msg1:
    fcc "hello, world!"
    fcb 0
msg2:
    fcc "this is rusty coco!"
    fcb 0
msg3:
    fcc "(press any key to exit)"
    fcb 0

* point the reset vector to our program
* note that coco hardware maps the vectors from $ffnn to $bfnn
	org $bffe
	fdb start

