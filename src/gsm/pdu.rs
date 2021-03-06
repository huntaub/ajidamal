extern crate chrono;

use self::chrono::prelude::*;
use super::errors::Error;
use std::str;
use std::mem;
use nom::IResult;
use nom;

//yyMMddHHMMss
const DATETIME_FORMAT_STRING: &'static str = "%y%m%d%H%M%S";

#[derive(Debug)]
enum AddressType {
    International, // 145
    ShortCode, // 201
}

#[derive(Debug)]
pub struct Number {
    format: AddressType,
    pub number: String,
}

impl Number {
    pub fn new_international(number: String) -> Number {
        Number {
            format: AddressType::International,
            number: number,
        }
    }

    fn serialize_to_pdu(&self, output: &mut Vec<u8>) {
        // Serialize the address-length
        let original_len = self.number.len() as u8;
        let mut length = original_len;
        let odd = length % 2 == 1;
        if odd {
            length += 1;
        }

        u8_to_hex(length, output);

        match self.format {
            AddressType::International => u8_to_hex(145, output),
            ref d => panic!("serialization not supported for address type {:?}", d)
        }

        let bytes = self.number.as_bytes();
        let mut seen: u8 = 0;
        while seen < original_len {
            if seen + 1 == original_len && odd {
                output.push(b'F');
            } else {
                output.push(bytes[(seen + 1) as usize]);
            }

            output.push(bytes[seen as usize]);

            seen += 2;
        }
    }
}

fn str_to_ascii(data: &str) -> u8 {
    assert!(data.len() == 1);
    data.as_bytes()[0]
}

fn u4_to_hex(data: u8) -> u8 {
    assert!(data <= 15);
    match data {
        0 => b'0',
        1 => b'1',
        2 => b'2',
        3 => b'3',
        4 => b'4',
        5 => b'5',
        6 => b'6',
        7 => b'7',
        8 => b'8',
        9 => b'9',
        10 => b'A',
        11 => b'B',
        12 => b'C',
        13 => b'D',
        14 => b'E',
        15 => b'F',
        _ => unreachable!()
    }
}

fn u8_to_hex(data: u8, output: &mut Vec<u8>) {
    let lower = data >> 4;
    let upper = data & 0b00001111;

    output.push(u4_to_hex(lower));
    output.push(u4_to_hex(upper));
}

#[derive(Debug)]
pub struct HeaderEntry {
    tag: u8,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct ConcatenatedMessage {
    pub reference_number: u8,
    pub number_of_messages: u8,
    pub sequence_number: u8,
}

named!(parse_concatenated_message<ConcatenatedMessage>,
       do_parse!(
           reference_number: map_res!(take!(2), u8_from_hex_str) >>
           number_of_messages: map_res!(take!(2), u8_from_hex_str) >>
           sequence_number: map_res!(take!(2), u8_from_hex_str) >>
           (ConcatenatedMessage {
               reference_number: reference_number,
               number_of_messages: number_of_messages,
               sequence_number: sequence_number,
           })
       )
);

#[derive(Debug)]
pub struct Header {
    pub concatenated_message: Option<ConcatenatedMessage>,
    entries: Vec<HeaderEntry>
}

impl Header {
    fn new() -> Header {
        Header {
            concatenated_message: None,
            entries: Vec::new(),

        }
    }

    fn set_entries(mut self, entries: Vec<HeaderEntry>) -> Self {
        self.entries = entries;
        self
    }

    fn parse_entries(&mut self) {
        let entries = mem::replace(&mut self.entries, Vec::new()).into_iter();

        for entry in entries.into_iter() {
            match entry.tag {
                0 => {
                    match parse_concatenated_message(&entry.data) {
                        IResult::Done(_, o) => {
                            self.concatenated_message.get_or_insert(o);
                            continue
                        },
                        a => {
                            println!("got failure parsing IEI {}: {:?}", entry.tag, a);
                        }
                    };

                    self.entries.push(entry);
                },
                _  => {
                    self.entries.push(entry);
                },
            }
        }
    }
}

#[derive(Debug)]
pub struct UserData {
    encoding: Encoding,
    pub data: String,
    pub header: Option<Header>
}

impl UserData {
    pub fn new_utf16(data: String) -> UserData {
        UserData {
            encoding: Encoding::Utf16,
            data: data,
            header: None,
        }
    }

    fn serialize_to_pdu(&self, output: &mut Vec<u8>) {
        assert!(self.header.is_none());
        assert!(self.encoding == Encoding::Utf16);

        let mut intermediate_output: Vec<u8> = Vec::new();
        let mut length = 0;
        for byte in self.data.encode_utf16() {
            u8_to_hex((byte >> 8) as u8, &mut intermediate_output);
            u8_to_hex((byte & 0b11111111) as u8, &mut intermediate_output);
            length += 2;
        }

        u8_to_hex(length as u8, output);
        output.extend(intermediate_output.into_iter());
    }
}

#[derive(Debug, PartialEq)]
pub enum ValidityPeriod {
    // Only relative validity periods are supported right now.
    // NotPresent, // 0 0
    Relative(u8), // 1 0
    // Enhanced, // 0 1
    // Absolute, // 1 1
}

#[derive(Debug)]
pub struct MessageSubmit {
    command_type: CommandInformation,
    reject_duplicates: bool,
    message_reference: u8,
    destination_address: Number,
    protocol_id: u8,
    user_data: UserData,
}

impl MessageSubmit {
    // Actually, we can get a new message reference with this vendor
    // by issuing AT+CMGENREF. Unclear if this standard functionality.

    pub fn new_default(reject_duplicates: bool, status_report_request: bool,
                       destination_address: Number, user_data: UserData) -> MessageSubmit {
        // Plain MO-MT messages have PID=0.
        Self::new(reject_duplicates, ValidityPeriod::Relative(255), status_report_request, /*reply_path=*/false,
                  destination_address, /*protocol_id=*/0, user_data)
    }

    pub fn new(reject_duplicates: bool, validity_period: ValidityPeriod, status_report_request: bool, reply_path: bool,
               destination_address: Number, protocol_id: u8, user_data: UserData) -> MessageSubmit {
        // Not quite sure what to do with the validity period
        // thing. Right now, only support TP-VP (relative format). The
        // supposition is to just set this to 255 (the maximum allowed
        // value).
        assert!(validity_period == ValidityPeriod::Relative(255));

        // TOOD: Add support for status reports.
        assert!(!status_report_request);

        // The internet seems to say that support for reply paths is
        // tenuous at best and is merely part of a plan to
        // reverse-charge for replies to this message. Let's not
        // support it.
        assert!(!reply_path);

        assert!(user_data.encoding == Encoding::Utf16);

        MessageSubmit {
            command_type: CommandInformation {
                message_type: MessageType::SmsSubmit,
                more_messages_to_send: false,
                has_udh: false,
            },
            reject_duplicates: reject_duplicates,
            protocol_id: protocol_id,
            message_reference: 0,
            destination_address: destination_address,
            user_data: user_data
        }
    }

    pub fn serialize_to_pdu(&self) -> Vec<u8> {
        // The first octet of the message contains the following bits:
        // 0/1 - MTI (set to 01 for SMS-SUBMIT)
        // 2 - Reject duplicates
        // 3/4 - Validity period format (set to 10 for relative)
        // 5 - Status report request (set to 0 for these messages)
        // 6 - User data header indicator (set to 0 for no header)
        // 7 - Reply path (set to 0)

        let mut first_octet: u8 = 0b00_01_00_01;
        if self.reject_duplicates {
            first_octet |= 0b1 << 2;
        }

        let mut output: Vec<u8> = Vec::new();
        u8_to_hex(first_octet, &mut output);
        u8_to_hex(0, &mut output);

        self.destination_address.serialize_to_pdu(&mut output);

        u8_to_hex(self.protocol_id, &mut output);

        // Encoding the data coding scheme as Utf16
        u8_to_hex(8, &mut output);

        // Serialize the validity period as 255
        u8_to_hex(0xFF, &mut output);

        self.user_data.serialize_to_pdu(&mut output);

        output
    }
}

#[derive(Debug)]
pub struct Message {
    service_center: Number,
    command_type: CommandInformation,
    pub sender: Number,
    pub time_stamp: DateTime<Utc>,
    protocol_id: u8,
    pub user_data: UserData,
}

#[derive(Debug)]
struct CommandInformation {
    message_type: MessageType,
    more_messages_to_send: bool,
    has_udh: bool,
}

#[derive(Debug)]
enum MessageType {
    SmsDeliverReport, // 0
    SmsDeliver, // 0
    SmsSubmit, // 1
    SmsSubmitReport, // 1
    SmsCommand, // 2
    SmsStatusReport, // 2
}

#[derive(Debug, PartialEq)]
enum Encoding {
    Gsm7Bit,
    Utf16,
    Unknown
}

fn u16_from_hex_str(data: &[u8]) -> Result<u16, Error> {
    str::from_utf8(data).or(Err(Error::ParseError)).and_then(|s| {
        u16::from_str_radix(s, 16).or(Err(Error::ParseError))
    })
}

fn u8_from_hex_str(data: &[u8]) -> Result<u8, Error> {
    str::from_utf8(data).or(Err(Error::ParseError)).and_then(|s| {
        u8::from_str_radix(s, 16).or(Err(Error::ParseError))
    })
}

fn str_from_decimal_octet(data: &[u8]) -> Result<String, Error> {
    let mut output = String::new();

    output.push(char::from(data[1]));

    if data[0] != b'F' {
        output.push(char::from(data[0]));
    }

    Ok(output)
}

fn concat_strings(data: Vec<String>) -> Result<String, Error> {
    Ok(data.into_iter().fold(String::new(), |acc, item| {
        acc + &item
    }))
}

fn get_decimal_length(data: u8) -> Result<u8, Error> {
    if data % 2 == 0 {
        Ok(data / 2)
    } else {
        Ok((data / 2) + 1)
    }
}

fn to_vec(data: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(data.to_vec())
}

// 1-0	TP-Message-Type-Indicator (TP-MTI)
//   2	TP-More-Messages-to-Send (TP-MMS) in SMS-DELIVER (0 = more messages)
//   2	TP-Reject-Duplicates (TP-RD) in SMS-SUBMIT
//   3	TP-Loop-Prevention (TP-LP) in SMS-DELIVER and SMS-STATUS-REPORT
// 4-3	TP-Validity-Period-Format (TP-VPF) in SMS-SUBMIT (00 = not present)
//   5	TP-Status-Report-Indication (TP-SRI) in SMS-DELIVER
//   5	TP-Status-Report-Request (TP-SRR) in SMS-SUBMIT and SMS-COMMAND
//   5	TP-Status-Report-Qualifier (TP-SRQ) in SMS-STATUS-REPORT
//   6	TP-User-Data-Header-Indicator (TP-UDHI)
//   7	TP-Reply-Path (TP-RP) in SMS-DELIVER and SMS-SUBMIT

fn to_command_information(data: u8) -> Result<CommandInformation, Error> {
    let message_type = match data & 0b11 {
        0 => MessageType::SmsDeliver,
        1 => MessageType::SmsDeliver,
        2 => MessageType::SmsSubmit,
        3 => MessageType::SmsCommand,
        d => {
            println!("got unexpected command type {:?}", d);
            return Err(Error::ParseError);
        }
    };

    let more_messages_to_send = match (data & 0b100) >> 2 {
        0 => true,
        1 => false,
        _ => panic!("bit wasn't 0 or 1")
    };

    let has_udh = match (data & 0b1000000) >> 6 {
        0 => false,
        1 => true,
        _ => panic!("bit wasn't 0 or 1")
    };

    Ok(CommandInformation{
        message_type: message_type,
        more_messages_to_send: more_messages_to_send,
        has_udh: has_udh,
    })
}

fn to_encoding_scheme(data: u8) -> Result<Encoding, Error> {
    match data {
        0 => Ok(Encoding::Gsm7Bit),
        8 => Ok(Encoding::Utf16),
        d => {
            println!("got unexpected encoding scheme {:?}", d);
            Err(Error::ParseError)
        }
    }
}

fn to_address_type(data: u8) -> Result<AddressType, Error> {
    match data {
        145 => Ok(AddressType::International), // International number + ISDN
        201 => Ok(AddressType::ShortCode), // Subscriber  number + private numbering
        d => {
            println!("got unexpected address type {:?}", d);
            Err(Error::ParseError)
        }
    }
}

fn parse_ascii_hex_number(data: u8) -> i32 {
    match data {
        48 => 0,
        49 => 1,
        50 => 2,
        51 => 3,
        52 => 4,
        53 => 5,
        54 => 6,
        55 => 7,
        56 => 8,
        57 => 9,
        // uppercase
        65 => 10,
        66 => 11,
        67 => 12,
        68 => 13,
        69 => 14,
        70 => 15,
        // lowercase
        97 => 10,
        98 => 11,
        99 => 12,
        100 => 13,
        101 => 14,
        102 => 15,
        // failure
        _ => 0,
    }
}

fn parse_time_zone(zero: i32, mut one: i32) -> i32 {
    let mut sign = 1;

    // The third bit stores the sign information. If it is 1, then the
    // time zone is negative. Ignore that bit and parse the number as
    // usual.
    if one & 0b1000 != 0 {
        one = one & 0b0111;
        sign = -1;
    }

    sign * ((10 * one) + zero)
}

fn parse_date_time(tz_data: &[u8], data: String) -> Result<DateTime<Utc>, Error> {

    let time_zone = parse_time_zone(parse_ascii_hex_number(tz_data[0]), parse_ascii_hex_number(tz_data[1]));

    let datetime = match FixedOffset::east(time_zone * 900)
                                     .datetime_from_str(data.as_ref(), DATETIME_FORMAT_STRING) {
        Ok(d) => d.with_timezone(&Utc),
        Err(e) => {
            println!("Got {:?} parsing the datetime", e);
            return Err(Error::ParseError)
        }
    };

    Ok(datetime)
}

fn parse_length(data: &[u8]) -> Result<u8, Error> {
    match u8_from_hex_str(data) {
        Ok(l) => Ok(2 * l),
        Err(e) => Err(e),
    }
}

named!(parse_iei_reserved<HeaderEntry>,
       dbg_dmp!( do_parse!(
           tag: map_res!(take!(2), u8_from_hex_str) >>
           data: map_res!(length_value!(map_res!(take!(2), parse_length),
                               nom::rest), to_vec) >>
           (HeaderEntry {
               tag: tag,
               data: data,
           })
       )));

named!(parse_user_header < Option < Header > > ,
       dbg_dmp!( do_parse!(
           entries: length_value!(map_res!(take!(2), parse_length),
                                  many0!(parse_iei_reserved)) >>
           (Some(Header::new().set_entries(entries)))
       ))
);

fn parse_user_data(data: &[u8], encoding: Encoding, length: u8, has_udh: bool) -> IResult<&[u8], UserData> {
    let original_len = data.len();

    // If the user data contains a UDH, then parse that before moving
    // on to the actual text.
    let (remaining, header) = if has_udh {
        match parse_user_header(data) {
            IResult::Done(i, o) => (i, o),
            IResult::Incomplete(n) => return IResult::Incomplete(n),
            IResult::Error(e) => return IResult::Error(e),
        }
    } else {
        (data, None)
    };

    let header = header.map(|mut e| {
        e.parse_entries();
        e
    });

    // Should have parsed an even number of u8s since the header would
    // be in octets.
    let parsed_octets = (original_len - remaining.len()) / 2;
    let remaining_length = length as usize - parsed_octets;

    match encoding {
        Encoding::Gsm7Bit => parse_gsm_alphabet(remaining, remaining_length).map(|parsed_data| {
            UserData {
                encoding: Encoding::Gsm7Bit,
                data: parsed_data,
                header: header,
            }
        }),
        Encoding::Utf16 => parse_utf16(remaining, remaining_length).map(|parsed_data| {
            UserData {
                encoding: Encoding::Utf16,
                data: parsed_data,
                header: header,
            }
        }),
        Encoding::Unknown => {
            IResult::Done(&remaining[remaining_length..],
                          UserData {
                              encoding: Encoding::Unknown,
                              data: format!("unknown encoding for data: {:?}", &data[..length as usize]),
                              header: header,
                          })
        }
    }
}

named!(hex_octet<u8>, map_res!(take!(2), u8_from_hex_str));

named!(decimal_octet<String>, map_res!(take!(2), str_from_decimal_octet));

named_args!(decimal_octet_number(length: u8)<String>,
            map_res!(
                count!(decimal_octet, length as usize),
                concat_strings));

named!(pub parse_pdu<Message>,
       do_parse!(
           sc_length: hex_octet >>
           sc_address_type: map_res!(hex_octet, to_address_type) >>
           service_center: apply!(decimal_octet_number, sc_length - 1) >>
           message_type: map_res!(hex_octet, to_command_information) >>
           sender_length: map_res!(hex_octet, get_decimal_length) >>
           sender_address_type: map_res!(hex_octet, to_address_type) >>
           sender: apply!(decimal_octet_number, sender_length) >>
           protocol_id: hex_octet >>
           encoding_scheme: map_res!(hex_octet, to_encoding_scheme) >>
           time_stamp: apply!(decimal_octet_number, 6) >>
           time_zone: take!(2) >>
           ud_length: hex_octet >>
           user_data: apply!(parse_user_data, encoding_scheme, ud_length, message_type.has_udh) >>

           (Message {
               service_center: Number {
                   format: sc_address_type,
                   number: service_center,
               },
               command_type: message_type,
               sender: Number {
                   format: sender_address_type,
                   number: sender,
               },
               protocol_id: protocol_id,
               time_stamp: parse_date_time(time_zone, time_stamp).unwrap(),
               user_data: user_data,
           })
       )
);

impl Message {
    pub fn from_string(pdu_string: String) -> Result<Message, ()> {
        match parse_pdu(pdu_string.as_bytes()) {
            IResult::Done(_, m) => {
                Ok(m)
            },
            IResult::Error(_) => Err(()),
            IResult::Incomplete(n) => {
                println!("incomplete? {:?}", n);
                Err(())
            }
        }
    }
}

const GSM_MASKS: &[u8] = &[
    0b01111111, // 1
    0b00111111, // 2
    0b00011111, // 3
    0b00001111, // 4
    0b00000111, // 5
    0b00000011, // 6
    0b00000001, // 7
    0b11111111, // 0
];

const GSM_CHARS: &[char] = &[
//   0     1     2     3     4     5     6     7     8     9     A     B     C      D    E     F
    '@',  '£',  '$',  '¥',  'è',  'é',  'ù',  'ì',  'ò',  'Ç', '\n',  'Ø',  'ø', '\r',  'Å',  'å', // 0
    'Δ',  '_',  'Φ',  'Γ',  'Λ',  'Ω',  'Π',  'Ψ',  'Σ',  'Θ',  'Ξ',  '?',  'Æ',  'æ',  'ß',  'É', // 1
    ' ',  '!',  '"',  '#',  '¤',  '%',  '&', '\'',  '(',  ')',  '*',  '+',  ',',  '-',  '.',  '/', // 2
    '0',  '1',  '2',  '3',  '4',  '5',  '6',  '7',  '8',  '9',  ':',  ';',  '<',  '=',  '>',  '?', // 3
    '¡',  'A',  'B',  'C',  'D',  'E',  'F',  'G',  'H',  'I',  'J',  'K',  'L',  'M',  'N',  'O', // 4
    'P',  'Q',  'R',  'S',  'T',  'U',  'V',  'W',  'X',  'Y',  'Z',  'Ä',  'Ö',  'Ñ',  'Ü',  '§', // 5
    '¿',  'a',  'b',  'c',  'd',  'e',  'f',  'g',  'h',  'i',  'j',  'k',  'l',  'm',  'n',  'o', // 6
    'p',  'q',  'r',  's',  't',  'u',  'v',  'w',  'x',  'y',  'z',  'ä',  'ö',  'ñ',  'ü',  'à'  // 7

];

fn parse_gsm_alphabet(pdu_string: &[u8], length: usize) -> IResult<&[u8], String> {
    let mut parsed_octets = 0;
    let mut output = String::new();
    let mut rest = pdu_string;

    let mut saved_byte: u8 = 0;
    while parsed_octets < length {
        let parse_stage = parsed_octets % 8;
        if parse_stage == 7 {
            output.push(GSM_CHARS[saved_byte as usize]);
            saved_byte = 0;
            parsed_octets += 1;
            continue;
        }

        let (new_rest, next_byte) = hex_octet(rest).unwrap();
        rest = new_rest;
        let character = (next_byte & GSM_MASKS[parse_stage as usize]) << parse_stage;

        output.push(GSM_CHARS[(character + saved_byte) as usize]);
        saved_byte = (next_byte & !GSM_MASKS[parse_stage as usize]) >> (7 - parse_stage);
        parsed_octets += 1;
    };

    IResult::Done(rest, output)
}

named!(u8_vec_to_u16_vec < &[u8], Vec<u16> >, many0!(
    map_res!(take!(4), u16_from_hex_str)));

fn parse_utf16(data: &[u8], length: usize) -> IResult<&[u8], String> {
    // length is in 16-bit groups, so we need to double it to get the full string
    let u16_len = length*2;
    if data.len() < u16_len {
        return IResult::Incomplete(nom::Needed::Size(u16_len - data.len()))
    }

    let utf16_str: Vec<u16> = u8_vec_to_u16_vec(&data[..u16_len]).to_result().unwrap();
    match String::from_utf16(utf16_str.as_ref()) {
        Ok(s) => IResult::Done(&data[u16_len..], s),
        Err(_) => IResult::Error(nom::ErrorKind::Custom(0))
    }
}

#[cfg(test)]
mod test {
    // TODO: Write some tests so that I don't have to worry so much
    // about regressions here.
}
