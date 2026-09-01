import { expect, describe, test } from "@jest/globals";
import { aircraft_information, airport_information } from "rs1090-wasm";

describe("rs1090 utils", () => {
  test("patterns", () => {
    const info = aircraft_information("39b415");
    expect(info.registration).toBe("F-HNAV");
    expect(info.country).toBe("France");

    const unknown = aircraft_information("000000");
    expect(unknown.country).toBe(undefined);
    expect(unknown.flag).toBe(undefined);
  });

  test("airports", () => {
    const a = airport_information("Paris");
    expect(a.length).toBe(11);
    expect(a.some((airport: { icao?: string }) => airport.icao === "LFPG")).toBe(true);
  });
});
