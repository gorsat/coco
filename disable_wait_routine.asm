* This is a patch meant for application after loading color basic.
* It nullifies the wait loop at A7D1 in order to speed up debugging
* of cartridges. The wait loop serves no purpose in the emulator.
	org $a7d1
	rts
