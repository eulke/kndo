package pkg

// Api is the module's published surface.
func Api() {}

// mentioned exists to prove the file is read at all: its imports and
// references stay evidence even where its declarations do not.
func mentioned() {}

func init() { mentioned() }
