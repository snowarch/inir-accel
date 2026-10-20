// Decodes each hex line of a corpus with the installed Go's encoding/json the way the VSCode themegen does (into
// map[string]any and []map[string]any) and prints the error and the compact re-encoding: the reference for the
// Rust gojsonv corpus test. Usage: go run ./test/gojson_corpus FILE > FILE.go
package main

import (
	"bufio"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
)

func main() {
	f, _ := os.Open(os.Args[1])
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1<<20), 1<<24)
	for sc.Scan() {
		b, _ := hex.DecodeString(sc.Text())
		m := map[string]any{}
		err := json.Unmarshal(b, &m)
		out, err2 := json.Marshal(m)
		enc := string(out)
		if err2 != nil {
			enc = err2.Error()
		}
		fmt.Printf("%v | %s\n", errText(err), enc)
		var s []map[string]any
		err = json.Unmarshal(b, &s)
		out, err2 = json.Marshal(s)
		enc = string(out)
		if err2 != nil {
			enc = err2.Error()
		}
		fmt.Printf("%v | %s\n", errText(err), enc)
	}
}

func errText(err error) string {
	if err == nil {
		return "<nil>"
	}
	return err.Error()
}
