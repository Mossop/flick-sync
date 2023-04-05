import { ReactNode, createContext, useContext } from "react";
import { View, StyleSheet, useWindowDimensions } from "react-native";
import * as Styles from "../modules/styles";

const styles = StyleSheet.create({
  outer: {
    flexDirection: "row",
    flexWrap: "wrap",
    paddingLeft: Styles.PADDING,
    paddingTop: Styles.PADDING,
  },
  item: {
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    paddingRight: Styles.PADDING,
    paddingBottom: Styles.PADDING,
  },
});

const GridContext = createContext(0);

export default function GridView({
  itemWidth,
  children,
}: {
  itemWidth: number;
  children: ReactNode;
}) {
  let { width } = useWindowDimensions();
  let count = Math.floor(
    (width - Styles.PADDING) / (itemWidth + Styles.PADDING),
  );
  let actualWidth = (width - Styles.PADDING) / count;

  return (
    <GridContext.Provider value={actualWidth}>
      <View style={styles.outer}>{children}</View>
    </GridContext.Provider>
  );
}

GridView.Item = function Item({ children }: { children: ReactNode }) {
  let actualWidth = useContext(GridContext);

  return <View style={[styles.item, { width: actualWidth }]}>{children}</View>;
};
