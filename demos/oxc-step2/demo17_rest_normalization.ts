type A<T extends unknown[]> = [...string[], ...T];
type B = A<number[]>;
type C = A<[boolean, null]>;
type D = A<[]>;
