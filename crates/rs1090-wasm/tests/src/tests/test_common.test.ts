import { decode } from "rs1090-wasm";
import { expect, describe, test } from "@jest/globals";

describe("Common Decoding Tests", () => {
  test.each([
    ["5d34530e459cfc", "II0"],
    ["5d34530e459cf3", "II15"],
    ["5d34530e459cec", "SI0"],
    ["5d34530e459ced", "SI1"],
    ["5d34530e459cb3", "SI63"],
    ["5d34530e459ca0", "invalid"],
    ["5d34530eba6303", "invalid"],
  ])("DF11 %s emits flat interrogator %s", (frame, expected) => {
    const msg = decode(frame);
    expect(msg.df).toBe("11");
    expect(msg.interrogator).toBe(expected);
    expect(msg.crc_syndrome).toBeUndefined();
  });

  test("crc", () => {
    expect(decode("8D406B902015A678D4D220AA4BDA").df).toBe("17");
    expect(decode("8d8960ed58bf053cf11bc5932b7d").df).toBe("17");
    expect(decode("8d45cab390c39509496ca9a32912").df).toBe("17");
    expect(decode("8d49d3d4e1089d00000000744c3b").df).toBe("17");
    expect(decode("8d74802958c904e6ef4ba0184d5c").df).toBe("17");
    expect(decode("8d4400cd9b0000b4f87000e71a10").df).toBe("17");
    expect(decode("8d4065de58a1054a7ef0218e226a").df).toBe("17");
  });

  test("fail crc with message", () => {
    expect(() => decode("8d4ca251204994b1c36e60a5343d")).toThrow(
      "Invalid CRC in ADS-B message: 16",
    );
  });

  test("icao24", () => {
    expect(decode("8D406B902015A678D4D220AA4BDA").icao24).toBe("406b90");
    expect(decode("A0001839CA3800315800007448D9").icao24).toBe("400940");
    expect(decode("A000139381951536E024D4CCF6B5").icao24).toBe("3c4dd2");
    expect(decode("A000029CFFBAA11E2004727281F1").icao24).toBe("4243d0");
  });

  test("altcode", () => {
    const msg = decode("A02014B400000000000000F9D514");
    expect(msg.df).toBe("20");
    expect(msg.altitude).toBe(32300);
  });

  test("idcode", () => {
    const msg = decode("A800292DFFBBA9383FFCEB903D01");
    expect(msg.df).toBe("21");
  });
});
