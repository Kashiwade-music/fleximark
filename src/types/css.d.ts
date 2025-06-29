// src/types/css.d.ts（または任意の場所）
declare module "*.css" {
  const content: string;
  export default content;
}
