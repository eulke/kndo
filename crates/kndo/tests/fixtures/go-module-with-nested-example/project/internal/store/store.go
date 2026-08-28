package store

type Store struct {
	items map[string]string
}

func New() *Store {
	return &Store{items: map[string]string{}}
}

func (s *Store) Put(key string, value string) {
	s.items[key] = value
}
