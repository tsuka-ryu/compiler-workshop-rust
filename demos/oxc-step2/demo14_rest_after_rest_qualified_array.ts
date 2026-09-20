declare namespace ns { type Array<T> = T[]; }
type A = [...string[], ...ns.Array<number>];
