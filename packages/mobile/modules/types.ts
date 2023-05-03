export type Replace<T, V> = Omit<T, keyof V> & V;
