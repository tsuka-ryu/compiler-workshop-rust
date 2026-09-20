type A<T> = T extends (infer U)[] ? U : T;
