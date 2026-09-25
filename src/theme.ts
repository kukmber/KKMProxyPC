import { BrandVariants, createDarkTheme, createLightTheme, Theme } from "@fluentui/react-components";

// Фирменный мятно-бирюзовый вместо стандартного синего Fluent.
const brand: BrandVariants = {
  10: "#020f0d",
  20: "#04201c",
  30: "#063028",
  40: "#073f34",
  50: "#084e40",
  60: "#095e4c",
  70: "#0a6e58",
  80: "#0b7f65",
  90: "#0c9072",
  100: "#10a27f",
  110: "#1fb38d",
  120: "#3dc39c",
  130: "#5fd1ad",
  140: "#83debf",
  150: "#a8ead2",
  160: "#cdf5e6",
};

export const lightTheme: Theme = createLightTheme(brand);

export const darkTheme: Theme = {
  ...createDarkTheme(brand),
  colorBrandForeground1: brand[120],
  colorBrandForeground2: brand[130],
  colorBrandForegroundLink: brand[120],
};
