package lib

type secret struct{ n int }

// Exported promises a type its consumers cannot name: the classic Go
// unexported-type-in-exported-signature leak. The body's own use of
// secret is NOT a second finding — only the signature is a promise.
func Exported(s secret) secret {
	inner := secret{n: s.n}
	return inner
}
