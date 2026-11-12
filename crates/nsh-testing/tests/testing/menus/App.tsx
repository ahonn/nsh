import { keymap, mount, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { Menu } from "nsh/desktop";

function App() {
  const chosen = signal("nothing");
  const typed = signal("");
  keymap.bind("mod+k", () => chosen.set("the keymap's K"));
  Menu.setApplicationMenu([
    {
      label: "File",
      submenu: [
        { label: "New", shortcut: "mod+n", onSelect: () => chosen.set("New") },
        { label: "Find", shortcut: "mod+e", when: "notEditing", onSelect: () => chosen.set("Find") },
        { label: "Save", shortcut: "mod+s", disabled: true, onSelect: () => chosen.set("Save") },
        { label: "Search", shortcut: "mod+k", onSelect: () => chosen.set("the menu's K") },
      ],
    },
  ]);
  return (
    <View style={{ flex: 1 }}>
      <Text>{`Chosen: ${chosen()}`}</Text>
      <Input testID="field" value={typed()} onChange={({ value }) => typed.set(value)} />
    </View>
  );
}

mount(App);
